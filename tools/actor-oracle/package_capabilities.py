"""Compare complete package capabilities with independent source/snapshot input.

Uses the existing independent native scalar/association and Python PACK readers.
Expected conditions are faithful adapter refusals, never condition truth or
engine observations. No observed Rust field contributes to an expected value.
"""
import argparse
import hashlib
import json
import pathlib
import struct
from dependencies import Reader, fields, key_tuple, plugin_name, require, MIB
from package_dependencies import executable_receipt, project

OBSERVATION_SCOPE = "Physical authored PKID order and PACK CTDA requests through the shared canonical condition adapter; explicit caller context, no actor-base association inference, effective package selection, condition truth/grouping, schedule, embedded-script execution or AI"
CAPABILITY_SCOPE = "Complete retained authored PACK fields and existing faithful condition/event/embedded-unit inputs; physical source completeness never admits eligibility, effective selection, schedule interpretation or AI execution"
DEPENDENCIES = {
    'eligibility': ['original_condition_truth', 'condition_grouping'],
    'selection': ['original_condition_truth', 'condition_grouping', 'effective_package_selection'],
    'scheduling': ['original_condition_truth', 'condition_grouping', 'effective_package_selection', 'schedule_word_interpretation', 'package_execution'],
}


def expected(reader, native, snapshot, root, subject, operation, descriptors):
    actor = next(row for row in native['definitions'] if key_tuple(row['key']) == root)
    require(not actor['deleted'], 'capability actor deleted')
    links = next(row['associations'] for row in native['actor_associations']['definitions'] if key_tuple(row['key']) == root)
    entry = reader.winners[root]
    body = reader.payload(root, 64 * MIB)
    require(hashlib.sha256(body).hexdigest() == actor['source']['decoded_record_sha256'], 'capability native actor body differs')
    raw = list(fields(body))
    require(len(raw) == len(actor['fields']), 'capability native actor physical fields differ')
    configurations = []
    for index, ((tag, offset, data), field) in enumerate(zip(raw, actor['fields'])):
        require(field['kind'] == list(tag.encode('latin1')) and field['decoded_offset'] == offset
                and field['bytes'] == len(data) and field['sha256'] == hashlib.sha256(data).hexdigest(), 'capability actor physical field differs')
        if tag == 'ACBS':
            require(len(data) == 24, 'capability ACBS extent')
            flags, template = struct.unpack_from('<I', data)[0], struct.unpack_from('<H', data, 22)[0]
            configuration = dict(field)
            configuration['value'] = dict(kind='actor_base', flags=flags, template_flags=template,
                                          inventory_template_flag=bool(template & 0x100))
            configurations.append([index, configuration])
    issues = []
    if len(configurations) == 1:
        configuration = configurations[0]
        if configuration[1]['value']['template_flags'] & 0x20:
            issues.append(dict(code='ai_package_template_selection_unsupported', inventory_field_index=configuration[0]))
    else:
        configuration = None
        issues.append(dict(code='missing_actor_configuration' if not configurations else 'ambiguous_actor_configuration', inventory_field_index=None))
    selected = []
    for link in links:
        if link['role'] != 'package':
            continue
        field = actor['fields'][link['field_index']]
        tag, _, data = raw[link['field_index']]
        require(tag == 'PKID' and len(data) == 4 and link['binding'] == reader.binding(entry['source'], struct.unpack('<I', data)[0]), 'capability native PKID link differs')
        selected.append((link, field))
    keys = {key_tuple(link['binding']['key']) for link, _ in selected
            if link['binding']['status'] == 'defined' and link['schema_kind_allowed'] is True}
    dependencies = {key_tuple(row['key']): row for row in project(reader, native, descriptors, keys)['definitions']}
    scalars = {key_tuple(row['key']): row for row in native['actor_packages']['definitions']}
    preparation_visits = len(raw) + len(links)
    condition_count = 0
    packages, inputs = [], []
    counts = dict(fields=len(raw), event_fields=0, scripts=0, script_entries=0, visits=0)
    origins = {reference['id']: reference['authored'] for reference in snapshot['references']}
    require(subject is None or subject in origins, 'capability subject absent from canonical snapshot')
    for occurrence_index, (link, field) in enumerate(selected):
        key = key_tuple(link['binding']['key'])
        package = dependencies.get(key) if key in keys else None
        source, sites = None, []
        input_row = dict(occurrence_index=occurrence_index, source=None, dependency_findings=[],
                         event_fields=[], scripts=[], physical_source_complete=False)
        if package is not None:
            prepared = package['conditions']
            source = dict(key=package['key'], source=package['source'], header=package['header'], condition_record=prepared['identity'])
            n = len(prepared['sites'])
            preparation_visits += n + n * (n + 1) // 2
            condition_count += n
            for site in prepared['sites']:
                function_id = struct.unpack_from('<H', bytes(site['raw_bytes']), 8)[0]
                reason = ('unverified_retail_semantics', 'Original condition return, comparison, subject and grouping rules are unverified') if function_id == 47 else (
                    'missing_implementation', 'Condition function has no replacement query adapter')
                sites.append(dict(campaign=snapshot['campaign'], source_cohort_sha256=snapshot['catalogue_sha256'],
                    state_revision=snapshot['state_revision'], source=prepared['identity'], site=site, explicit_subject=subject,
                    intent='faithful', outcome=dict(status='unsupported', reason=reason[0], detail=reason[1]),
                    condition_truth=None, condition_evaluation_ready=False, original_behavior_verified=False))
            scalar = scalars[key]
            input_row = dict(occurrence_index=occurrence_index, source=scalar, dependency_findings=package['findings'],
                             event_fields=package['event_fields'], scripts=package['scripts'], physical_source_complete=True)
            entries = sum(len(script['declarations']) + len(script['references']) + len(script['issues']) for script in package['scripts'])
            counts['fields'] += len(scalar['fields'])
            counts['event_fields'] += len(package['event_fields'])
            counts['scripts'] += len(package['scripts'])
            counts['script_entries'] += entries
            counts['visits'] += len(scalar['fields']) + len(scalar['findings']) + len(package['findings']) + len(package['event_fields']) + len(package['scripts']) + entries
        packages.append(dict(association=link, field=field, package=source, conditions=sites, eligible=None))
        inputs.append(input_row)
    counts['visits'] += preparation_visits + len(raw) + len(actor['findings']) + len(issues)
    observation = dict(campaign=snapshot['campaign'], source_cohort_sha256=snapshot['catalogue_sha256'],
        state_revision=snapshot['state_revision'], actor_key=actor['key'], actor_source=actor['source'],
        configuration=configuration, issues=issues, explicit_subject=subject,
        explicit_subject_origin=origins.get(subject), intent='faithful', packages=packages,
        condition_requests=condition_count, preparation_visits=preparation_visits, query_contributions=0,
        effective_package_selection_supported=False, scheduling_supported=False, original_behavior_verified=False, scope=OBSERVATION_SCOPE)
    return dict(observation=observation, actor_fields=actor['fields'], actor_findings=actor['findings'], package_inputs=inputs,
        physical_source_complete=all(row['physical_source_complete'] for row in inputs), counts=counts,
        refusal=dict(operation=operation, dependencies=DEPENDENCIES[operation]), execution_admitted=False,
        state_changed=False, scope=CAPABILITY_SCOPE)


def load(path):
    raw = path.read_bytes()
    require(len(raw) <= 256 * MIB, 'capability oracle input byte budget')
    return json.loads(raw), dict(path=str(path), bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'native-report', 'snapshot', 'observed-report', 'executable', 'output']:
        parser.add_argument('--' + name, type=pathlib.Path, required=True)
    parser.add_argument('--actor-root', required=True)
    parser.add_argument('--explicit-subject', type=int)
    parser.add_argument('--operation', choices=list(DEPENDENCIES), required=True)
    parser.add_argument('--team-directory', type=pathlib.Path)
    parser.add_argument('--session-id')
    args = parser.parse_args()

    def guard():
        if args.team_directory is None:
            return
        read = lambda path: json.loads(path.read_text(encoding='utf-8-sig'))
        team = args.team_directory
        control, assignment, lease = read(team.parent / 'team/control.json'), read(team / 'actors.assignment.json'), read(team / 'leases/actors.json')
        require(control['mode'] == 'active' and not control['stop_requested'] and assignment['state'] == 'active'
                and assignment['implementation_authorized'] and control['run_id'] == assignment['run_id'] == lease['run_id']
                and control['generation'] == assignment['generation'] and lease['session_uuid'] == args.session_id, 'capability oracle authorization changed')
        for mailbox in team.glob('*.outbox.jsonl'):
            for line in mailbox.read_text(encoding='utf-8-sig').splitlines():
                if line.strip():
                    row = json.loads(line)
                    require(row.get('run_id') != control['run_id'] or row.get('type') != 'stop_requested', 'capability oracle observed STOP')
    guard()
    native, native_receipt = load(args.native_report)
    snapshot, snapshot_receipt = load(args.snapshot)
    observed, observed_receipt = load(args.observed_report)
    names = json.loads(args.load_order.read_text(encoding='utf-8-sig'))
    require(isinstance(names, list) and 0 < len(names) <= 256, 'capability load order')
    origin, local = args.actor_root.split(':')
    root = plugin_name(origin), int(local, 16)
    require(0 < root[1] <= 0xFFFFFF, 'capability root')
    reader = Reader(args.data, names, native, guard)
    try:
        wanted = expected(reader, native, snapshot, root, args.explicit_subject, args.operation, executable_receipt(args.executable))
        actual = observed.get('package_capability', observed)
        mismatched = sorted(name for name in set(wanted) | set(actual) if wanted.get(name) != actual.get(name))
        guard()
        result = dict(complete_capability_equal=not mismatched, mismatched_fields=mismatched,
            native_input=native_receipt, snapshot_input=snapshot_receipt, observed_input=observed_receipt,
            original_input_receipts=[dict(path=str(s.path), bytes=s.size, sha256=s.sha256) for s in reader.sources],
            operation=args.operation, physical_source_complete=wanted['physical_source_complete'], counts=wanted['counts'],
            original_behavior_verified=False, state_changed=False)
        with args.output.open('x', encoding='utf-8') as output:
            json.dump(result, output, indent=2)
        if mismatched:
            raise SystemExit('package capability comparison differs: ' + ', '.join(mismatched))
        print(json.dumps(dict(complete_capability_equal=True, operation=args.operation, counts=wanted['counts'])))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
