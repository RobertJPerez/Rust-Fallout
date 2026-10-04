"""Independent explicit canonical lot selection; snapshot input is not authority.

Reuse original source-header Reader. Facts come only from the supplied snapshot,
never from observed Rust output or an inferred equip/mod rule.
"""
import argparse
import json
import pathlib
from dependencies import Reader,key_tuple,require,MIB

SCOPE='One explicit canonical owner/item lot and exact optional facts with winning source header; inventory presence/slot/modification metadata never chooses equipped state, active weapon mod mask, model role, attachment or retail item rules'


def expected(reader,snapshot,owner,item):
    require(snapshot['schema_version']==4,'strict current snapshot')
    references=[r for r in snapshot['references'] if r['id']==owner]
    require(len(references)==1,'canonical owner')
    banks=[b for b in snapshot['inventory_banks'] if b['owner']==owner]
    require(len(banks)==1,'explicit initialized inventory')
    items=sorted(banks[0]['items'],key=lambda lot:lot['id'])
    matches=[i for i,lot in enumerate(items) if lot['id']==item and lot['owner']==owner]
    require(len(matches)==1,'selected canonical lot')
    usage=dict(items=len(items),links=0,extra_bytes=0)
    for lot in items:
        facts=lot['facts']
        usage['links']+=len(facts['equipped_slots'] or [])+len(facts['modifications'] or [])+len(facts['extra_fields'])
        usage['links']+=sum(facts[n] is not None for n in ['ammo','script_instance','ownership'])
        usage['extra_bytes']+=sum(len(field['bytes']) for field in facts['extra_fields'])
    entry=reader.winners[key_tuple(items[matches[0]]['facts']['base'])]
    require(not entry['flags']&0x20,'base source deleted')
    return dict(inventory=dict(campaign=snapshot['campaign'],catalogue_sha256=snapshot['catalogue_sha256'],
        state_revision=snapshot['state_revision'],boundary=snapshot['clocks'],owner=owner,authored=references[0]['authored'],items=items,usage=usage),
        selected_item_index=matches[0],source_form=dict(kind=entry['kind'],flags=entry['flags']),visits=len(items)+2,
        equip_rules_supported=False,active_mod_mask=None,scope=SCOPE)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for n in ['data','load-order','base-report','snapshot','observed','output']:parser.add_argument('--'+n,type=pathlib.Path,required=True)
    parser.add_argument('--owner',type=int,required=True);parser.add_argument('--item',type=int,required=True)
    parser.add_argument('--team-directory',type=pathlib.Path);parser.add_argument('--session-id')
    args=parser.parse_args()
    def guard():
        if args.team_directory is None:return
        team=args.team_directory;read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
        control,assignment,lease=read(team.parent/'team/control.json'),read(team/'actors.assignment.json'),read(team/'leases/actors.json')
        require(control['mode']=='active' and not control['stop_requested'] and assignment['state']=='active' and assignment['implementation_authorized']
            and control['run_id']==assignment['run_id']==lease['run_id'] and control['generation']==assignment['generation']
            and lease['session_uuid']==args.session_id,'lot oracle authorization changed')
        for path in team.glob('*.outbox.jsonl'):
            for line in path.read_text(encoding='utf-8-sig').splitlines():
                row=json.loads(line);require(row.get('run_id')!=control['run_id'] or row.get('type')!='stop_requested','STOP')
    guard()
    for p,limit in [(args.base_report,256*MIB),(args.snapshot,32*MIB),(args.observed,256*MIB)]:require(p.stat().st_size<=limit,'input byte budget')
    native=json.loads(args.base_report.read_bytes());snapshot=json.loads(args.snapshot.read_bytes());observed=json.loads(args.observed.read_bytes())
    reader=Reader(args.data,json.loads(args.load_order.read_text(encoding='utf-8-sig')),native,guard)
    try:
        result=expected(reader,snapshot,args.owner,args.item)
        require(result==observed.get('equipment_item',observed),'complete independent lot selection differs')
        with args.output.open('x',encoding='utf-8') as target:json.dump(result,target,separators=(',',':'))
        print(json.dumps(dict(equal=True,owner=args.owner,item=args.item,index=result['selected_item_index'],usage=result['inventory']['usage'],retail_parity_accepted=False)))
    finally:reader.close()


if __name__=='__main__':main()
