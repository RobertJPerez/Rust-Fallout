"""Independent actor placement/base join and supplied canonical snapshot view.

Reuses existing native scalar/placement and raw source readers. Snapshot input
supplies expected canonical facts; this oracle never restores an authoritative World.
"""
import argparse
import hashlib
import json
import pathlib
import struct
from dependencies import Reader, fields, key_tuple, require, MIB

SCOPE = 'Exact authored ACHR/ACRE placement and winning NPC_/CREA base joined to one existing canonical reference; source transform/flags separate from optional current pose/enable, no registration, actor initialization, defaults, template evaluation or state mutation'


def physical(reader, key, definition):
    body=reader.payload(key,64*MIB)
    require(hashlib.sha256(body).hexdigest()==definition['source']['decoded_record_sha256'],'native body hash')
    raw=list(fields(body))
    require(len(raw)==len(definition['fields']),'physical field count')
    for (tag,offset,data),field in zip(raw,definition['fields']):
        require(field['kind']==list(tag.encode('latin1')) and field['decoded_offset']==offset
            and field['bytes']==len(data) and field['sha256']==hashlib.sha256(data).hexdigest(),'physical field origin')
    return raw


def expected(reader,native,snapshot,reference):
    require(snapshot['schema_version']==4,'strict current snapshot fixture')
    refs=[r for r in snapshot['references'] if r['id']==reference]
    require(len(refs)==1 and refs[0]['authored'] is not None,'explicit authored reference')
    origin=key_tuple(refs[0]['authored'])
    placed=next(r for r in native['actor_placements']['definitions'] if key_tuple(r['key'])==origin)
    require(not placed['deleted'],'placed source deleted')
    entry=reader.winners[origin]
    raw=physical(reader,origin,placed)
    bases=[i for i,r in enumerate(raw) if r[0]=='NAME']
    transforms=[i for i,r in enumerate(raw) if r[0]=='DATA']
    require(len(bases)==len(transforms)==1,'placed core singleton')
    binding=reader.binding(entry['source'],struct.unpack('<I',raw[bases[0]][2])[0])
    require(binding==placed['base'] and binding['status']=='defined','exact base binding')
    key=key_tuple(binding['key'])
    actor=next(r for r in native['definitions'] if key_tuple(r['key'])==key)
    require(not actor['deleted'] and bytes(actor['kind'])==(b'NPC_' if entry['kind_name']=='ACHR' else b'CREA'),'actor kind')
    actor_raw=physical(reader,key,actor)
    words=struct.unpack('<6I',raw[transforms[0]][2])
    require(placed['core']['position_bits']==list(words[:3]) and placed['core']['rotation_bits']==list(words[3:]),'exact transform bits')
    states=[r['state'] for r in snapshot['reference_states'] if r['id']==reference]
    require(len(states)<=1,'canonical state unique')
    count=len(raw)+len(actor_raw)
    return dict(reference=dict(campaign=snapshot['campaign'],catalogue_sha256=snapshot['catalogue_sha256'],
        revision=snapshot['state_revision'],reference=reference,authored=refs[0]['authored'],state=states[0] if states else None),
        placement=placed,actor={n:actor[n] for n in ['key','source','kind','record_version','fields','findings']},
        base_field_index=bases[0],transform_field_index=transforms[0],fields=count,
        visits=2*len(native['sources'])+count+7,actor_initialization_supported=False,authored_enable_evaluated=False,scope=SCOPE)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for n in ['data','load-order','base-report','snapshot','observed','output']:parser.add_argument('--'+n,type=pathlib.Path,required=True)
    parser.add_argument('--reference',type=int,required=True)
    parser.add_argument('--team-directory',type=pathlib.Path)
    parser.add_argument('--session-id')
    args=parser.parse_args()
    def guard():
        if args.team_directory is None:return
        team=args.team_directory
        read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
        control,assignment,lease=read(team.parent/'team/control.json'),read(team/'actors.assignment.json'),read(team/'leases/actors.json')
        require(control['mode']=='active' and not control['stop_requested'] and assignment['state']=='active' and assignment['implementation_authorized']
            and control['run_id']==assignment['run_id']==lease['run_id'] and control['generation']==assignment['generation']
            and lease['session_uuid']==args.session_id,'oracle authorization changed')
        for path in team.glob('*.outbox.jsonl'):
            for line in path.read_text(encoding='utf-8-sig').splitlines():
                row=json.loads(line)
                require(row.get('run_id')!=control['run_id'] or row.get('type')!='stop_requested','STOP')
    guard()
    for p,limit in [(args.base_report,256*MIB),(args.snapshot,32*MIB),(args.observed,256*MIB)]:require(p.stat().st_size<=limit,'input byte budget')
    native=json.loads(args.base_report.read_bytes());snapshot=json.loads(args.snapshot.read_bytes());observed=json.loads(args.observed.read_bytes())
    reader=Reader(args.data,json.loads(args.load_order.read_text(encoding='utf-8-sig')),native,guard)
    try:
        result=expected(reader,native,snapshot,args.reference)
        require(result==observed.get('actor_context',observed),'complete independent actor context differs')
        with args.output.open('x',encoding='utf-8') as target:json.dump(result,target,separators=(',',':'))
        print(json.dumps(dict(equal=True,fields=result['fields'],visits=result['visits'],base=result['actor']['key'],current_state_available=result['reference']['state'] is not None,retail_parity_accepted=False)))
    finally:reader.close()


if __name__=='__main__':main()
