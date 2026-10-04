"""Independent CREA model-list directory requests; no NIF or engine base inference."""
from dependencies import key_tuple, normalize, require, render_manifest


def manifest(reader, root, definitions, structural, directory, content_digest):
    require(reader.winners[root]['kind_name'] == 'CREA' and not definitions[root]['deleted'], 'creature root')
    directory = normalize(directory)
    require(directory.startswith(b'meshes/') and len(directory) <= 4096, 'explicit creature directory')
    render = render_manifest(reader, root, definitions, structural,
        dict(sources=4096, requests=16384, issues=16384, visits=2000000), content_digest)
    selected = {request['manifest_path_index']: request['ambiguous_source'] for request in render['requests']}
    counts = dict(requests=0, lookup_attempts=0, lookup_path_bytes=0,
        aggregate_candidates=structural['counts']['candidates'], aggregate_candidate_bytes=structural['counts']['candidate_bytes'],
        visits=len(render['requests']) + len(structural['paths']))
    requests = []
    for index, source_path in enumerate(structural['paths']):
        reader.guard()
        if key_tuple(source_path['source']) != root or source_path['role'] != 'model_list': continue
        config = render['configuration']
        if config is None: selection = 'actor_configuration_unavailable'
        elif config['template_flags'] & 64: selection = 'model_template_selection_unsupported'
        else:
            require(index in selected, 'model frame missing selected role')
            selection = 'ambiguous_source' if selected[index] else 'authored_source'
        lookup, status, candidates = None, None, []
        if selection == 'authored_source':
            counts['lookup_attempts'] += 1
            raw = bytes(source_path['raw'])
            if not raw: status = 'empty_source_path'
            elif len(raw) > 4096: status = 'lookup_path_too_long'
            else:
                try:
                    normalize(raw)
                    if len(directory[7:]) + 1 + len(raw) > 4096: status = 'lookup_path_too_long'
                    else:
                        lookup = normalize(directory + b'/' + raw)
                        candidates = reader.mounts.get(lookup, [])
                        status = ('missing_archive_candidate' if not candidates else 'one_archive_candidate' if len(candidates) == 1 else 'archive_collision')
                except ValueError: status = 'unsafe_asset_path'
            if lookup is not None: counts['lookup_path_bytes'] += len(lookup)
            counts['aggregate_candidates'] += len(candidates)
            counts['aggregate_candidate_bytes'] += sum(len(c['container'].encode('utf-8')) + len(c['original_path']) for c in candidates)
        requests.append(dict(manifest_path_index=index, selection=selection, lookup_status=status,
            asset_path=None if lookup is None else list(lookup), candidates=candidates))
    counts['requests'] = len(requests)
    require(counts['requests'] <= 16384 and counts['visits'] <= 2000000 and counts['lookup_path_bytes'] <= 32 * 1024 * 1024
        and counts['aggregate_candidates'] <= 100000 and counts['aggregate_candidate_bytes'] <= 32 * 1024 * 1024, 'creature request budgets')
    admitted = bool(requests) and not render['issues'] and not render['selected_source_cycles'] and all(
        r['selection'] == 'authored_source' and r['lookup_status'] == 'one_archive_candidate' for r in requests)
    result = dict(render=render, explicit_mesh_directory=list(directory), directory_authority='explicit_caller_directory',
        requests=requests, counts=counts, part_requests_admitted=admitted,
        effective_part_selection_supported=False, rig_playback_supported=False,
        scope='Physical CREA NIFZ frames requested under an explicit caller mesh directory; original structural/render requests unchanged, no inferred relative base, effective part assembly, NIF decoding, rig timing or retail selection')
    import json
    require(len(json.dumps(result, separators=(',', ':'), ensure_ascii=False).encode('utf-8')) <= 32 * 1024 * 1024, 'creature projection budget')
    return result
