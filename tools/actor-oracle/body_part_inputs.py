"""Independent physical PNAM/BPTD candidate source declarations.

The established locked plugin/header/FormID reader and native actor/inventory
producers supply framing and ACBS inputs. No nearest-name grouping or limb choice.
"""
import argparse
import collections
import hashlib
import json
import pathlib
import struct
from dependencies import Reader, MIB, fields, key_tuple, key_json, require
from context import physical

SCOPE = 'Exact creature PNAM and physical BPTD names/BPND part declarations, signed part/actor-value/count words, raw hit/float/flag/unused bytes and winning debris/explosion/impact/ragdoll headers. ModelAnimation inheritance and repeated/unknown source declarations remain explicit. No nearest-name grouping, editor sorting, contact/limb matching, model choice, damage, limb health, gore/dismemberment evaluation or runtime mutation'
HEADER = ('kind','offset','stored_size','flags','form_id','revision','version','trailing_bytes')
NAMES = {'BPTN','BPNN','BPNT','BPNI','NAM1','NAM4','MODL'}


def source(reader, key):
    h=reader.winners[key]
    s=reader.sources[h['source']]
    return dict(key=key_json(key),source_name=reader.names[h['source']],source_bytes=s.size,
        source_sha256=s.sha256,header={n:h[n] for n in HEADER})


def manifest(reader, native, inventory, root):
    aa=[a for a in native['definitions'] if key_tuple(a['key'])==root]
    ii=[a for a in inventory['definitions'] if key_tuple(a['key'])==root]
    require(len(aa)==len(ii)==1,'one exact creature/inventory source')
    actor,inv=aa[0],ii[0]
    h=reader.winners[root]
    require(h['kind_name']=='CREA' and not h['flags']&0x20 and actor['source']==inv['source'],'live exact creature')
    require(actor['kind']==h['kind'] and actor['record_version']==h['version'] and not actor['deleted']
        and actor['source']['plugin']==reader.names[h['source']]
        and actor['source']['sha256']==reader.sources[h['source']].sha256
        and actor['source']['record_file_offset']==h['offset'] and actor['source']['record_flags']==h['flags'], 'exact creature winning source origin')
    actor_raw=physical(reader,root,actor)
    physical(reader,root,inv)
    configs=[f for f in inv['fields'] if f['kind']==list(b'ACBS')]
    mask=configs[0]['value']['template_flags'] if len(configs)==1 and configs[0]['value']['kind']=='actor_base' else None
    supported=actor['record_version']==15
    occurrences=sum(tag=='PNAM' for tag,_,_ in actor_raw)
    c=dict(decoded_bytes=len(reader.payload(root,MIB)),field_visits=len(inv['fields'])+2*len(actor_raw),
        fields=len(actor_raw),parts=0,names=0,name_bytes=0,raw_bytes=0,bindings=0,headers=1)
    m=dict(sources=native['sources'],winning_content_sha256=native['winning_content_sha256'],actor=actor,
        configuration_fields=configs,model_animation_template_flag=None if mask is None else bool(mask&64),
        actor_record_version_supported=supported,pnam_fields=[],pnam_links=[],pnam_repeated=occurrences>1,
        declaration_binding_available=False,declaration=None,counts=c,issues=[],contact_mapped=False,
        damage_evaluated=False,dismemberment_supported=False,scope=SCOPE)
    if not supported:m['issues'].append('unsupported_creature_record_version')
    if mask is None:m['issues'].append('unique_configuration_unavailable')
    if mask is not None and mask&64:m['issues'].append('model_animation_template_inheritance_unsupported')
    if not occurrences:m['issues'].append('body_part_link_absent')
    if occurrences>1:m['issues'].append('repeated_body_part_link')

    def retained(tag,offset,raw):
        c['raw_bytes']+=len(raw)
        return dict(kind=list(tag.encode('latin1')),decoded_offset=offset,raw_bytes=list(raw),sha256=hashlib.sha256(raw).hexdigest())

    def bound(source_index,index,offset,word,role,allowed):
        c['bindings']+=1
        b=reader.binding(source_index,word)
        key=key_tuple(b['key'])
        s=source(reader,key) if key in reader.winners else None
        if s:c['headers']+=1
        permit=None if b['target'] is None else bytes(b['target']['kind']).decode('latin1')==allowed
        return dict(field_index=index,field_byte_offset=offset,role=role,binding=b,source=s,
            schema_kind_allowed=permit,structural_binding_available=b['status']=='defined' and permit is True)

    for index,(tag,offset,raw) in enumerate(actor_raw):
        if tag!='PNAM':continue
        m['pnam_fields'].append(retained(tag,offset,raw))
        if not supported or len(raw)!=4:
            m['issues'].append('unsupported_body_part_link_layout')
            continue
        m['pnam_links'].append(bound(h['source'],index,0,struct.unpack('<I',raw)[0],'body_part','BPTD'))
    chosen=m['pnam_links']
    selected=key_tuple(chosen[0]['binding']['key']) if len(chosen)==1 and occurrences==1 and chosen[0]['structural_binding_available'] else None
    m['declaration_binding_available']=selected is not None and m['model_animation_template_flag'] is False
    if selected:
        c['headers']+=1
        winner=reader.winners[selected]
        body=reader.payload(selected,MIB)
        c['decoded_bytes']+=len(body)
        version=winner['version']==15
        d=dict(source=source(reader,selected),decoded_record_sha256=hashlib.sha256(body).hexdigest(),
            record_version_supported=version,fields=[],name_field_indices=[],parts=[],links=[],
            source_grouping_verified=False,issues=[])
        if not version:d['issues'].append('unsupported_body_part_record_version')
        for index,(tag,offset,raw) in enumerate(fields(body)):
            reader.guard()
            c['field_visits']+=1;c['fields']+=1
            d['fields'].append(retained(tag,offset,raw))
            if tag in NAMES:
                c['names']+=1;c['name_bytes']+=len(raw)
                d['name_field_indices'].append(index)
            if tag=='BPND':
                c['parts']+=1
                known=version and len(raw)==84
                n=None
                if known:
                    u32=lambda at:struct.unpack_from('<I',raw,at)[0]
                    part,actor_value=struct.unpack_from('<b',raw,5)[0],struct.unpack_from('<b',raw,7)[0]
                    n=dict(damage_mult_bits=u32(0),flags=raw[4],flags_known=raw[4]&128==0,
                        part_type=part,part_type_known=-1<=part<=14,health_percent=raw[6],actor_value=actor_value,
                        actor_value_known=-1<=actor_value<=76,to_hit_chance=raw[8],explosion_chance=raw[9],
                        explodable_debris_count=struct.unpack_from('<H',raw,10)[0],tracking_max_angle_bits=u32(20),
                        explodable_debris_scale_bits=u32(24),severable_debris_count=struct.unpack_from('<i',raw,28)[0],
                        severable_debris_scale_bits=u32(40),gore_position_bits=list(struct.unpack_from('<3I',raw,44)),
                        gore_rotation_bits=list(struct.unpack_from('<3I',raw,56)),severable_decal_count=raw[76],
                        explodable_decal_count=raw[77],unused=list(raw[78:80]),limb_replacement_scale_bits=u32(80))
                    for at,role,allowed in [(12,'explodable_debris','DEBR'),(16,'explodable_explosion','EXPL'),
                        (32,'severable_debris','DEBR'),(36,'severable_explosion','EXPL'),
                        (68,'severable_impact_dataset','IPDS'),(72,'explodable_impact_dataset','IPDS')]:
                        c['field_visits']+=1
                        d['links'].append(bound(winner['source'],index,at,u32(at),role,allowed))
                d['parts'].append(dict(field_index=index,layout_supported=known,data=n,duplicate_part_type=False))
            elif tag=='RAGA' and version and len(raw)==4:
                d['links'].append(bound(winner['source'],index,0,struct.unpack('<I',raw)[0],'ragdoll','RGDL'))
        repeated=collections.Counter(p['data']['part_type'] for p in d['parts'] if p['data'] is not None)
        c['field_visits']+=2*len(d['parts'])
        for p in d['parts']:
            p['duplicate_part_type']=p['data'] is not None and repeated[p['data']['part_type']]>1
        m['declaration']=d
    require(len(native['sources'])<=256 and c['decoded_bytes']<=8*MIB and c['field_visits']<=200000 and c['fields']<=65536
        and c['parts']<=4096 and c['names']<=4096 and c['name_bytes']<=MIB and c['raw_bytes']<=2*MIB
        and c['bindings']<=4096 and c['headers']<=4096,'body-part request budgets')
    require(len(json.dumps(m,separators=(',',':')).encode())<=16*MIB,'projection budget')
    return m


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['data','load-order','base-report','inventory-report','output']:p.add_argument('--'+n,type=pathlib.Path,required=True)
    p.add_argument('--root',required=True)
    p.add_argument('--observed',type=pathlib.Path)
    p.add_argument('--team-directory',type=pathlib.Path)
    p.add_argument('--session-id')
    a=p.parse_args()
    def guard():
        if a.team_directory is None:return
        team=a.team_directory;read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
        c,x,l=read(team.parent/'team/control.json'),read(team/'actors.assignment.json'),read(team/'leases/actors.json')
        require(c['mode']=='active' and not c['stop_requested'] and x['state']=='active' and x['implementation_authorized']
            and c['run_id']==x['run_id']==l['run_id'] and c['generation']==x['generation']==l['generation'] and l['session_uuid']==a.session_id,'actor authorization changed')
        for box in team.glob('*.outbox.jsonl'):
            for line in box.read_text(encoding='utf-8-sig').splitlines():
                r=json.loads(line);require(r.get('run_id')!=c['run_id'] or r.get('type')!='stop_requested','STOP')
    guard()
    for path in [a.base_report,a.inventory_report]:require(path.stat().st_size<=256*MIB,'native input budget')
    native,inv=json.loads(a.base_report.read_bytes()),json.loads(a.inventory_report.read_bytes())
    require(native['sources']==inv['sources'] and native['winning_content_sha256']==inv['metadata']['winning_definitions_sha256'],'exact native cohorts')
    name,id_=a.root.rsplit(':',1)
    reader=Reader(a.data,json.loads(a.load_order.read_text(encoding='utf-8-sig')),native,guard)
    try:
        m=manifest(reader,native,inv,(name.lower(),int(id_,16)))
        if a.observed:
            require(a.observed.stat().st_size<=256*MIB,'observed input budget')
            observed=json.loads(a.observed.read_bytes())
            observed=observed['actor_body_part_inputs']['manifest'] if 'actor_body_part_inputs' in observed else observed
            require(observed==m,'complete independent body-part source projection differs')
        native['actor_body_part_inputs']=dict(manifest=m)
        guard()
        with a.output.open('x',encoding='utf-8') as f:json.dump(native,f,separators=(',',':'))
        print(json.dumps(dict(equal=True if a.observed else None,root=a.root,counts=m['counts'],retail_parity_accepted=False)))
    finally:reader.close()


if __name__=='__main__':main()
