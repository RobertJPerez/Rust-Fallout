"""Independent physical PACK destination unions over existing locked-source Reader.

Native PACK projection supplies its independently decoded scalar/header receipts;
union operands come from original bytes, never an observed production request.
"""
import argparse
import hashlib
import json
import pathlib
import struct
from dependencies import MIB, Reader, fields, key_tuple, require

SCOPE = 'Exact physical PACK location/target operands; signed source words and optional float bits, unique live schema-permitted FormID bindings only; no selected destination, radius fallback, search, pathfinding, schedule/condition truth or AI execution'
SLOTS = {'PLDT': 0, 'PLD2': 1, 'PTDT': 2, 'PTD2': 3}
REFERENCES = {'REFR', 'PGRE', 'PMIS', 'PBEA', 'ACHR', 'ACRE', 'PLYR'}
OBJECTS = {'ACTI', 'DOOR', 'STAT', 'FURN', 'CREA', 'SPEL', 'NPC_', 'CONT', 'ARMO', 'AMMO', 'MISC', 'WEAP', 'BOOK', 'KEYM', 'ALCH', 'LIGH', 'CHIP', 'CMNY', 'CCRD', 'IMOD'}


def manifest(reader, native, root):
    entry = reader.winners[root]
    require(entry['kind_name'] == 'PACK' and not entry['flags'] & 0x20, 'live PACK root')
    packages = [p for p in native['actor_packages']['definitions'] if key_tuple(p['key']) == root]
    require(len(packages) == 1, 'one independent physical package')
    package = packages[0]
    require(package['header'] == reader.header(reader.sources[entry['source']].read(entry['offset'], 24), entry['offset']), 'independent PACK header')
    body = reader.payload(root, 64 * MIB)
    require(hashlib.sha256(body).hexdigest() == package['source']['decoded_record_sha256'], 'independent physical body hash')
    physical = list(fields(body))
    require(len(physical) == len(package['fields']) <= 200000, 'destination field budget')
    counts = {tag: sum(f[0] == tag for f in physical) for tag in SLOTS}
    require(sum(counts.values()) <= 4096, 'destination operand budget')
    operands, total, supported = [], 0, True
    for index, (tag, offset, raw) in enumerate(physical):
        reader.guard()
        declared = package['fields'][index]
        require(declared['kind'] == list(tag.encode('ascii')) and declared['decoded_offset'] == offset
            and declared['bytes'] == len(raw) and declared['sha256'] == hashlib.sha256(raw).hexdigest(), 'independent physical field span/hash')
        if tag not in SLOTS:
            continue
        total += len(raw)
        require(total <= MIB, 'operand raw byte budget')
        slot = SLOTS[tag]
        row = dict(field_index=index, field_kind=list(tag.encode('ascii')), field_decoded_offset=offset,
            raw_bytes=list(raw), discriminant=None, union_word=None, signed_scalar=None, unknown_float_bits=None,
            alternative='unsupported_layout', binding=None, schema_kind_allowed=None, object_type_known=None,
            repeated=counts[tag] > 1, binding_admitted=False, findings=[])
        if row['repeated']:
            row['findings'].append('repeated_destination_operand')
        valid = len(raw) == 12 if slot < 2 else len(raw) in {12, 16}
        if valid:
            selector, word, signed = struct.unpack_from('<iIi', raw)
            row.update(discriminant=selector, union_word=word, signed_scalar=signed)
            if len(raw) == 16:
                row['unknown_float_bits'] = struct.unpack_from('<I', raw, 12)[0]
            choices = {0: 'reference', 1: 'cell', 2: 'unused', 3: 'unused', 4: 'object_id', 5: 'object_type', 6: 'unused', 7: 'unused'} if slot < 2 else {0: 'reference', 1: 'object_id', 2: 'object_type', 3: 'unused'}
            alternative = row['alternative'] = choices.get(selector, 'unknown')
            if alternative in {'reference', 'cell', 'object_id'}:
                binding = row['binding'] = reader.binding(entry['source'], word)
                target = binding['target']
                permitted = REFERENCES if alternative == 'reference' else {'CELL'} if alternative == 'cell' else OBJECTS | ({'LVLN', 'LVLC', 'FACT', 'FLST'} if slot >= 2 else set()) | ({'IDLM'} if slot == 2 else set())
                row['schema_kind_allowed'] = None if target is None else bytes(target['kind']).decode('ascii') in permitted
                if binding['status'] != 'defined':
                    row['findings'].append({'null': 'null_destination_form', 'missing': 'missing_destination_form', 'deleted': 'deleted_destination_form'}[binding['status']])
                if row['schema_kind_allowed'] is False:
                    row['findings'].append('destination_form_kind_not_allowed')
                row['binding_admitted'] = not row['repeated'] and binding['status'] == 'defined' and row['schema_kind_allowed'] is True
            elif alternative == 'object_type':
                row['object_type_known'] = word <= 28
                if word > 28:
                    row['findings'].append('unknown_destination_object_type')
            elif alternative == 'unused':
                row['findings'].append('destination_context_unavailable')
            else:
                row['findings'].append('unknown_destination_discriminant')
        else:
            row['findings'].append('unsupported_destination_layout')
        if not valid or row['alternative'] == 'unknown' or row['object_type_known'] is False:
            supported = False
        operands.append(row)
    result = dict(sources=native['sources'], winning_content_sha256=native['winning_content_sha256'], package=package,
        operands=operands, field_visits=len(physical), operand_bytes=total, source_layouts_supported=supported,
        path_target_selected=False, execution_supported=False,
        issues=[] if operands else ['no_authored_destination_operands'], scope=SCOPE)
    require(len(json.dumps(result, separators=(',', ':'), ensure_ascii=False).encode('utf-8')) <= 8 * MIB, 'projection budget')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'base-report', 'output']:
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
        read = lambda p: json.loads(p.read_text(encoding='utf-8-sig'))
        control, assignment, lease = read(team.parent / 'team/control.json'), read(team / 'actors.assignment.json'), read(team / 'leases/actors.json')
        require(control['mode'] == 'active' and not control['stop_requested'] and assignment['state'] == 'active'
            and assignment['implementation_authorized'] and control['run_id'] == assignment['run_id'] == lease['run_id']
            and control['generation'] == assignment['generation'] and lease['session_uuid'] == args.session_id, 'destination authorization changed')
        for mailbox in team.glob('*.outbox.jsonl'):
            for line in mailbox.read_text(encoding='utf-8-sig').splitlines():
                r = json.loads(line)
                require(r.get('run_id') != control['run_id'] or r.get('type') != 'stop_requested', 'STOP')
    guard()
    require(args.base_report.stat().st_size <= 256 * MIB, 'native input byte budget')
    native = json.loads(args.base_report.read_bytes())
    origin, local = args.root.split(':')
    root = origin.lower(), int(local, 16)
    reader = Reader(args.data, json.loads(args.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        result = manifest(reader, native, root)
        if args.observed:
            require(args.observed.stat().st_size <= 256 * MIB, 'observed input byte budget')
            observed = json.loads(args.observed.read_bytes())
            observed = observed['actor_package_destination']['manifest'] if 'actor_package_destination' in observed else observed
            require(result == observed, 'complete independent destination projection differs')
        native['actor_package_destination'] = dict(manifest=result)
        with args.output.open('x', encoding='utf-8') as target:
            json.dump(native, target, separators=(',', ':'))
        print(json.dumps(dict(equal=True if args.observed else None, root=args.root, operands=len(result['operands']),
            bytes=result['operand_bytes'], field_visits=result['field_visits'], retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
