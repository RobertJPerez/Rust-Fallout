"""Second authored external packet forest with literal simultaneous matrices.

Only the independent source byte writer is reused. No parser, matrix product,
interpolator, packet-selection guess or original playback algorithm is copied.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import check_clip as writer

NULL = writer.NULL
NAMES = [b'Leaf\0\xff', b'Sib\x80', b'Other', b'Arm', b'MainRoot', b'OtherRoot', b'Unselected', b'NiTransformController']
ID = [1,0,0,0,1,0,0,0,1]
R90 = [0,-1,0,1,0,0,0,0,1]
RM90 = [0,1,0,-1,0,0,0,0,1]
R180 = [-1,0,0,0,-1,0,0,0,1]


def container(names, packets, types, roots):
    return writer.container(names, packets, types, NAMES)[:-8] + writer.words(len(roots), *roots)


def skeleton_packets():
    packets = [
        writer.node(0, [], [99,98,97], R180, 7, 7),
        writer.node(1, [], [98,97,96], RM90, 9, 8),
        writer.node(2, [], [97,96,95], R90, 8, 9),
        writer.node(3, [0,1], [777,888,999], R90, 9, 10),
        writer.node(4, [3], [-4,6,2], RM90, 3),
        writer.node(5, [2], [10,-5,7], R180, 2),
        writer.node(6, [], [1000,2000,3000], ID, 99, 7),
    ]
    for target in [0,1,2,3]:
        packets.append(writer.words(NULL) + struct.pack('<H', 0x004c) + writer.floats(17,-9,100,101) + writer.words(target,NULL))
    return packets


def skeleton(packets=None, roots=(4,5)):
    return container(['NiNode','NiTransformController'], packets or skeleton_packets(), [0]*7+[1]*4, roots)


def keys(first, last, scales):
    return writer.words(0,2,1) + writer.floats(-2,*first,2,*last) + writer.words(2,1) + writer.floats(-2,scales[0],2,scales[1])


def clip_packets():
    sequence = writer.words(NULL,4,9)
    for ordinal, (name, priority) in enumerate([(3,47),(0,13),(1,91),(2,3)]):
        sequence += writer.words(1+2*ordinal,NULL) + bytes([priority]) + writer.words(name,NULL,7,NULL,NULL)
    sequence += writer.floats(-3) + writer.words(NULL,2) + writer.floats(13,100,101) + writer.words(NULL,NULL) + struct.pack('<H',0)
    packets = [sequence]
    for ordinal, (first,last,scales) in enumerate([
        ([-2,1,3],[2,5,-1],[-2,2]),
        ([1,-2,0],[3,2,4],[1,3]),
        ([-3,2,1],[1,0,3],[2,4]),
        ([2,1,-1],[-2,3,1],[-1,1]),
    ]):
        packets += [writer.floats(100,200,300,2,3,4,5,16)+writer.words(2+2*ordinal), keys(first,last,scales)]
    return packets


def clip(packets=None):
    return container(['NiControllerSequence','NiTransformInterpolator','NiTransformData'], packets or clip_packets(), [0,1,2,1,2,1,2,1,2], (0,))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def time_bits(value):
    return struct.unpack('<Q',struct.pack('<d',value))[0]


def channels(other, child, parent, sibling):
    return [dict(object=obj,node_name_bytes=list(NAMES[obj]),sequence=0,controlled_ordinal=packet,source_time=time)
            for obj,packet,time in [(2,3,other),(0,1,child),(3,0,parent),(1,2,sibling)]]


# Caller order is other root's child, main child, shared parent, main sibling.
CASES = [
    ('first',channels(-2,-2,-2,-2),[
        [[0,1,0,2],[-1,0,0,1],[0,0,-1,-1]],
        [[-1,0,0,1],[0,-1,0,-2],[0,0,1,0]],
        [[0,2,0,-2],[-2,0,0,1],[0,0,-2,3]],
        [[0,2,0,-3],[-2,0,0,2],[0,0,2,1]],
    ],[
        [[0,-2,0,6],[2,0,0,-7],[0,0,-2,5]],
        [[6,0,0,-7],[0,6,0,24],[0,0,-6,11]],
        [[-6,0,0,-1],[0,-6,0,12],[0,0,-6,11]],
        [[0,-12,0,17],[12,0,0,0],[0,0,-12,5]],
    ]),
    ('zero-parent',channels(0,0,0,0),[
        [[0,0,0,0],[0,0,0,2],[0,0,0,0]],
        [[-2,0,0,2],[0,-2,0,0],[0,0,2,2]],
        [[0,0,0,0],[0,0,0,3],[0,0,0,1]],
        [[0,3,0,-1],[-3,0,0,1],[0,0,3,2]],
    ],[
        [[0,0,0,10],[0,0,0,-9],[0,0,0,7]],
        [[0,0,0,5],[0,0,0,6],[0,0,0,5]],
        [[0,0,0,5],[0,0,0,6],[0,0,0,5]],
        [[0,0,0,5],[0,0,0,6],[0,0,0,5]],
    ]),
    ('last',channels(2,2,2,2),[
        [[0,-1,0,-2],[1,0,0,3],[0,0,1,1]],
        [[-3,0,0,3],[0,-3,0,2],[0,0,3,4]],
        [[0,-2,0,2],[2,0,0,5],[0,0,2,-1]],
        [[0,4,0,1],[-4,0,0,0],[0,0,4,3]],
    ],[
        [[0,2,0,14],[-2,0,0,-11],[0,0,2,9]],
        [[-18,0,0,29],[0,-18,0,12],[0,0,18,23]],
        [[6,0,0,11],[0,6,0,0],[0,0,6,-1]],
        [[0,24,0,17],[-24,0,0,0],[0,0,24,17]],
    ]),
    ('mixed',channels(2,2,-2,0),[
        [[0,-1,0,-2],[1,0,0,3],[0,0,1,1]],
        [[-3,0,0,3],[0,-3,0,2],[0,0,3,4]],
        [[0,2,0,-2],[-2,0,0,1],[0,0,-2,3]],
        [[0,3,0,-1],[-3,0,0,1],[0,0,3,2]],
    ],[
        [[0,2,0,14],[-2,0,0,-11],[0,0,2,9]],
        [[18,0,0,-19],[0,18,0,0],[0,0,-18,-13]],
        [[-6,0,0,-1],[0,-6,0,12],[0,0,-6,11]],
        [[0,-18,0,5],[18,0,0,6],[0,0,-18,-1]],
    ]),
]


def check(binary, prior_binary, output):
    binary=binary.resolve();prior_binary=prior_binary.resolve();output.mkdir(parents=True,exist_ok=False)
    si=output/'skeleton-inputs';ci=output/'clip-inputs';ri=output/'requests'
    for folder in [si,ci,ri]:folder.mkdir()
    s=si/'authored.nif';s.write_bytes(skeleton());c=ci/'authored.kf';c.write_bytes(clip())
    frozen={str(p):sha(p) for p in [binary,prior_binary,s,c]};invocations=[];literals=[];permutations=[];refusals=[];equal=[];strict=[]
    def request(name,value):
        p=ri/(name+'.json');p.write_text(json.dumps(value)+'\n');frozen[str(p)]=sha(p);return p
    def invoke(exe,name,args,code=0,destination=None,no_report=False):
        dest=destination or output/(name+'.json');command=[str(exe),*args,'--output',str(dest)]
        p=subprocess.run(command,capture_output=True,text=True);(output/(name+'.stderr.txt')).write_text(p.stderr)
        assert (p.returncode != 0 if no_report else p.returncode==code),(name,p.stderr)
        if no_report:assert not dest.exists();value=None
        else:value=json.loads(dest.read_text());assert value['failures']==code,value
        invocations.append(dict(command=command,exit_code=p.returncode,receipt_sha256=sha(dest) if dest.exists() else None));return value,dest
    def run(name,rows,skeleton_path=s,clip_path=c,error=None,patch=None):
        value=dict(schema_version=1,expected_skeleton_sha256=list(bytes.fromhex(sha(skeleton_path))),expected_clip_sha256=list(bytes.fromhex(sha(clip_path))),channels=rows)
        value.update(patch or {});p=request(name,value)
        report,_=invoke(binary,name,['nif-clip-pose',str(skeleton_path),str(clip_path),'--set','--request',str(p)],int(error is not None))
        if error:
            assert report['evaluation'] is None and report['error'],report
            if error is not True:assert error in report['error'],report
            refusals.append(name);return None
        result=report['evaluation'];assert result['skeleton_sha256']==sha(skeleton_path) and result['clip_sha256']==sha(clip_path)
        assert [o['channel']['object']['block'] for o in result['objects']]==[r['object'] for r in rows]
        assert [o['channel']['requested_time_f64_bits'] for o in result['objects']]==[time_bits(r['source_time']) for r in rows]
        assert (result['scene_decodes'],result['animation_key_decodes'],result['whole_source_sha256_computations'],result['additional_span_hashes'],result['propagated_objects'])==(1,1,2,20,6)
        assert not result['retail_behavior_verified'];return result
    def verify_span(observed,path,packets,roots):
        # The independent writer supplies lengths. No production parser enters
        # these offsets or exact byte-slice digests.
        start=path.stat().st_size-(4+4*len(roots))-sum(map(len,packets));block=observed['block']
        offset=start+sum(map(len,packets[:block]));size=len(packets[block]);data=path.read_bytes()
        assert observed==dict(block=block,offset=offset,bytes=size,sha256=hashlib.sha256(data[offset:offset+size]).hexdigest()),observed
    for name,rows,locals_,worlds in CASES:
        value=run(name,rows)
        assert [o['channel']['local'] for o in value['objects']]==locals_,name
        assert [o['source_world'] for o in value['objects']]==worlds,name
        for object_,row in zip(value['objects'],rows):
            channel=object_['channel'];assert channel['node_name_bytes']==row['node_name_bytes']
            assert channel['controlled_packet']==dict(interpolator=1+2*row['controlled_ordinal'],controller=None,priority=[47,13,91,3][row['controlled_ordinal']],node_name=row['object'],property_type=None,controller_type=7,controller_id=None,interpolator_id=None)
            assert channel['unapplied_sequence_fields']['weight_bits']==struct.unpack('<I',writer.floats(-3))[0]
            assert channel['unapplied_sequence_fields']['frequency_bits']==struct.unpack('<I',writer.floats(13))[0]
            assert channel['unapplied_interpolator_fields']['rotation_wxyz_bits']==[struct.unpack('<I',writer.floats(v))[0] for v in [2,3,4,5]]
            verify_span(channel['object'],s,skeleton_packets(),(4,5))
            for field in ['sequence','interpolator','data']:verify_span(channel[field],c,clip_packets(),(0,))
            for ancestor in object_['ancestors']:verify_span(ancestor['source'],s,skeleton_packets(),(4,5))
        for ordinal in [1,3]:
            assert [(a['source']['block'],a['applied_object']) for a in value['objects'][ordinal]['ancestors']]==[(3,3),(4,None)]
            assert value['objects'][ordinal]['ancestors'][0]['effective_local']==locals_[2]
        literals.append(name)
        reverse=run(name+'-reverse',list(reversed(rows)))
        assert reverse['objects']==list(reversed(value['objects']))
        for field in ['retained_bytes','work_units','sample_work','propagated_objects']:assert reverse[field]==value[field]
        permutations.append(name)
        # A parent with static ancestors still has the same complete old single
        # receipt. Child requests remain old explicit controlled-ancestor errors.
        row=rows[2];p=request(name+'-old',dict(schema_version=1,expected_skeleton_sha256=list(bytes.fromhex(sha(s))),expected_clip_sha256=list(bytes.fromhex(sha(c))),**row))
        args=['nif-clip-pose',str(s),str(c),'--request',str(p)]
        old,op=invoke(prior_binary,name+'-prior',args);new,np=invoke(binary,name+'-preserved',args);assert op.read_bytes()==np.read_bytes()
        prior=old['evaluation'];channel=value['objects'][2]['channel']
        assert all(channel[field]==prior[field] for field in channel),name
        assert value['objects'][2]['source_world']==prior['source_world'];equal.append(name)
    signed=channels(-0.0,0,0,0);run('negative-zero-time',signed)
    rows=channels(0,0,0,0)
    run('missing-parent',[rows[0],rows[1],rows[3]],error='required ancestor 3 controller 10')
    run('duplicate-destination',rows+[rows[0]],error='duplicate clip pose set destination')
    bad=copy.deepcopy(rows);bad[0]['controlled_ordinal']=bad[1]['controlled_ordinal'];run('duplicate-packet',bad,error='duplicate clip pose set controlled packet')
    for name,patch in [('stale-skeleton',dict(expected_skeleton_sha256=[0]*32)),('stale-clip',dict(expected_clip_sha256=[0]*32))]:run(name,rows,error='SHA256 differs',patch=patch)
    for name,patch in [('late-time',dict(source_time=3)),('late-name',dict(node_name_bytes=[1,2,3])),('late-sequence',dict(sequence=1)),('late-ordinal',dict(controlled_ordinal=4))]:
        bad=copy.deepcopy(rows);bad[-1].update(patch);run(name,bad,error=True)
    for name,mutate in [('ambiguous-name',lambda p:p.__setitem__(6,writer.node(0,[],[0,0,0],ID,1))),('absent-name',lambda p:p.__setitem__(2,writer.node(NULL,[],[0,0,0],R90,1,9)))]:
        packets=skeleton_packets();mutate(packets);path=si/(name+'.nif');path.write_bytes(skeleton(packets));frozen[str(path)]=sha(path);run(name,rows,skeleton_path=path,error=True)
    path=si/'unreachable.nif';path.write_bytes(skeleton(roots=(5,)));frozen[str(path)]=sha(path);run('unreachable',rows,skeleton_path=path,error='not reachable from footer')
    for name,offset,replacement in [('property',25+3*29,writer.words(0)),('wrong-type',29+3*29,writer.words(6)),('missing-packet-name',21+3*29,writer.words(NULL))]:
        packets=clip_packets();p=bytearray(packets[0]);p[offset:offset+4]=replacement;packets[0]=bytes(p)
        path=ci/(name+'.kf');path.write_bytes(clip(packets));frozen[str(path)]=sha(path);run(name,rows,clip_path=path,error=True)
    packets=clip_packets();packets[8]=writer.words(1,1)+writer.floats(0,1,0,0,0)+writer.words(0,0)
    path=ci/'rotation.kf';path.write_bytes(clip(packets));frozen[str(path)]=sha(path);run('rotation',rows,clip_path=path,error='rotation key mapping is unapplied')
    base=dict(schema_version=1,expected_skeleton_sha256=list(bytes.fromhex(sha(s))),expected_clip_sha256=list(bytes.fromhex(sha(c))),channels=rows)
    variants=[dict(base,extra=1),dict(base,schema_version=2),dict(base,channels=[]),dict(base,channels=rows*65)]
    v=copy.deepcopy(base);v['channels'][0]['extra']=1;variants.append(v)
    v=copy.deepcopy(base);del v['channels'][0]['node_name_bytes'];variants.append(v)
    v=copy.deepcopy(base);v['channels'][0]['source_time']=float('inf');variants.append(v)
    v=copy.deepcopy(base);del v['expected_clip_sha256'];variants.append(v)
    for ordinal,v in enumerate(variants):
        name=f'strict-{ordinal}';p=request(name,v);invoke(binary,name,['nif-clip-pose',str(s),str(c),'--set','--request',str(p)],no_report=True);strict.append(name)
    p=request('guard',base)
    for name,destination in [('protected-skeleton',si/'blocked.json'),('protected-clip',ci/'blocked.json'),('protected-request',ri/'blocked.json')]:
        invoke(binary,name,['nif-clip-pose',str(s),str(c),'--set','--request',str(p)],destination=destination,no_report=True);strict.append(name)
    invoke(binary,'set-batch-conflict',['nif-clip-pose',str(s),str(c),'--set','--batch','--request',str(p)],no_report=True);strict.append('set-batch-conflict')
    for path,digest in frozen.items():assert sha(Path(path))==digest,path
    result=dict(contract='engineering-explicit-external-clip-pose-set-v1',literal_cases=literals,permutation_cases=permutations,prior_complete_reports_byte_equal=equal,
                semantic_refusals=refusals,strict_and_protected_refusals=strict,invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--prior-binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();result=check(args.binary,args.prior_binary,args.output)
    print(json.dumps({k:v for k,v in result.items() if k not in ('invocations','frozen_hashes')}))
