"""Independent actor SCRI request using existing native projections and helpers.

Actor and inventory definitions come from their separate existing C++ producers.
Compiled unit metadata uses package_dependencies.script_unit, which independently
implements the already pinned unit/handle format. No observed field is expected.
"""
import argparse
import hashlib
import json
import pathlib
import struct
from dependencies import Reader, MIB, fields, key_tuple, require
from package_dependencies import SCRIPT_FIELDS, script_unit

SCOPE = 'Exact authored actor SCRI fields and existing source-bound standalone compiled definitions; source handles/version/owner/declarations/references only, no template inheritance, event-list replacement, live instance, event dispatch or execution'


def request(reader, native, inventory, root):
    actors = [d for d in native['definitions'] if key_tuple(d['key']) == root]
    inventories = [d for d in inventory['definitions'] if key_tuple(d['key']) == root]
    require(len(actors) == len(inventories) == 1, 'one independent actor/inventory source')
    actor, source = actors[0], inventories[0]
    entry = reader.winners[root]
    require(entry['kind_name'] in {'NPC_', 'CREA'} and not entry['flags'] & 0x20, 'live actor')
    body = reader.payload(root, 64 * MIB)
    require(actor['source'] == source['source'], 'independent actor/inventory identity')
    require(actor['source']['decoded_record_sha256'] == hashlib.sha256(body).hexdigest()
        and actor['source']['plugin'] == reader.names[entry['source']]
        and actor['source']['sha256'] == reader.sources[entry['source']].sha256
        and actor['source']['record_file_offset'] == entry['offset'] and actor['source']['record_flags'] == entry['flags'], 'actor fresh body/header identity')
    physical = list(fields(body))
    require(len(physical) == len(actor['fields']) == len(source['fields']), 'actor physical field count')
    for (tag, offset, raw), declared in zip(physical, actor['fields']):
        require(declared['kind'] == list(tag.encode('latin1')) and declared['decoded_offset'] == offset
            and declared['bytes'] == len(raw) and declared['sha256'] == hashlib.sha256(raw).hexdigest(), 'actor physical field/hash')
    configuration = [f for f in source['fields'] if f['kind'] == list(b'ACBS')]
    template = None
    if len(configuration) == 1 and configuration[0]['value']['kind'] == 'actor_base':
        template = bool(configuration[0]['value']['template_flags'] & 512)
    count = sum(tag == 'SCRI' for tag, _, _ in physical)
    require(count <= 4096 and 2 * len(physical) + len(source['fields']) <= 200000, 'selected physical work budget')
    attachments, total = [], 0
    for index, (tag, offset, raw) in enumerate(physical):
        if tag != 'SCRI':
            continue
        total += len(raw)
        require(total <= MIB, 'attachment byte budget')
        binding = reader.binding(entry['source'], struct.unpack('<I', raw)[0]) if len(raw) == 4 else None
        target = None if binding is None else binding['target']
        attachments.append(dict(field_index=index, field_decoded_offset=offset, raw_bytes=list(raw), binding=binding,
            schema_kind_allowed=None if target is None else target['kind'] == list(b'SCPT'), repeated=count > 1))
    definitions, declarations, references = [], 0, 0
    status = 'no_script_field'
    if len(attachments) > 1:
        status = 'multiple_script_fields'
    elif attachments:
        attachment = attachments[0]
        binding = attachment['binding']
        if binding is None:
            status = 'unsupported_script_layout'
        elif binding['status'] != 'defined':
            status = {'null': 'null_script', 'missing': 'missing_script', 'deleted': 'deleted_script'}[binding['status']]
        elif attachment['schema_kind_allowed'] is False:
            status = 'wrong_script_kind'
        else:
            key = key_tuple(binding['key'])
            target = reader.winners[key]
            payload = reader.payload(key, 64 * MIB)
            units = []
            for index, (tag, offset, raw) in enumerate(fields(payload)):
                if tag not in SCRIPT_FIELDS:
                    continue
                if tag == 'SCHR':
                    require(len(raw) == 20 and len(units) < 4096, 'standalone unit/header budget')
                    units.append(dict(index=index, offset=offset, marker=None, fields=[]))
                require(units, 'script field owner')
                units[-1]['fields'].append((tag, offset, raw))
            for unit in units:
                decoded = script_unit(reader, key, target, hashlib.sha256(payload).hexdigest(), unit)
                decoded.pop('field_index')
                decoded.pop('physical_marker')
                decoded['owner'] = dict(kind='standalone', section_marker=None, stage_marker=None, schema_ownership_verified=len(units) == 1)
                declarations += len(decoded['declarations'])
                references += len(decoded['references'])
                require(declarations <= 262144 and references <= 1000000, 'metadata projection budget')
                definitions.append(decoded)
            status = 'missing_loaded_definition' if not definitions else 'multiple_standalone_units' if len(definitions) > 1 else 'missing_compiled_body' if definitions[0]['version']['compiled_sha256'] is None else 'loaded_definition'
    if status == 'loaded_definition':
        status = 'missing_configuration' if not configuration else 'multiple_configurations' if len(configuration) > 1 else 'unsupported_configuration' if template is None else 'template_inheritance_unsupported' if template else 'loaded_definition'
    result = dict(sources=native['sources'], winning_content_sha256=native['winning_content_sha256'], actor=actor,
        configuration_fields=configuration, template_script_flag=template, attachments=attachments, compiled_definitions=definitions,
        selected_definition=0 if status == 'loaded_definition' else None, status=status,
        field_visits=2 * len(physical) + len(source['fields']), attachment_bytes=total, declarations=declarations,
        references=references, execution_supported=False, scope=SCOPE)
    require(len(json.dumps(result, separators=(',', ':'), ensure_ascii=False).encode('utf-8')) <= 16 * MIB, 'request projection budget')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'base-report', 'inventory-report', 'output']:
        parser.add_argument('--' + name, type=pathlib.Path, required=True)
    parser.add_argument('--root', required=True)
    parser.add_argument('--observed', type=pathlib.Path)
    parser.add_argument('--team-directory', type=pathlib.Path)
    parser.add_argument('--session-id')
    args = parser.parse_args()

    def guard():
        if args.team_directory is None:
            return
        team = args.team_directory
        control = json.loads((team.parent / 'team/control.json').read_text(encoding='utf-8-sig'))
        assignment = json.loads((team / 'actors.assignment.json').read_text(encoding='utf-8-sig'))
        lease = json.loads((team / 'leases/actors.json').read_text(encoding='utf-8-sig'))
        require(control.get('mode') == 'active' and not control.get('stop_requested')
            and assignment.get('state') == 'active' and assignment.get('implementation_authorized')
            and control.get('generation') == assignment.get('generation') == lease.get('generation')
            and control.get('run_id') == assignment.get('run_id') == lease.get('run_id')
            and lease.get('session_uuid') == args.session_id, 'actor run/lease changed')
        for box in team.glob('*.outbox.jsonl'):
            for line in box.read_text(encoding='utf-8-sig').splitlines():
                row = json.loads(line)
                require(row.get('run_id') != control['run_id'] or row.get('type') != 'stop_requested', 'current-run STOP')

    guard()
    require(args.base_report.stat().st_size <= 256 * MIB and args.inventory_report.stat().st_size <= 256 * MIB, 'native input byte budget')
    native = json.loads(args.base_report.read_bytes())
    inventory = json.loads(args.inventory_report.read_bytes())
    require(native['sources'] == inventory['sources'] and native['winning_content_sha256'] == inventory['metadata']['winning_definitions_sha256'], 'independent native cohorts differ')
    origin, local = args.root.rsplit(':', 1)
    reader = Reader(args.data, json.loads(args.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        result = request(reader, native, inventory, (origin.lower(), int(local, 16)))
        if args.observed:
            require(args.observed.stat().st_size <= 256 * MIB, 'observed input byte budget')
            observed = json.loads(args.observed.read_bytes())
            observed = observed['actor_script_attachment']['request'] if 'actor_script_attachment' in observed else observed
            require(result == observed, 'complete independent script attachment differs')
        native['actor_script_attachment'] = dict(request=result)
        with args.output.open('x', encoding='utf-8') as target:
            json.dump(native, target, separators=(',', ':'))
        print(json.dumps(dict(equal=True if args.observed else None, root=args.root, status=result['status'],
            units=len(result['compiled_definitions']), declarations=result['declarations'], references=result['references'], retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
