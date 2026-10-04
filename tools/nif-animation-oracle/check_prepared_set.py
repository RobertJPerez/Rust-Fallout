"""Independent higher-parent source/literal matrices through the prepared CLI.

Only existing binary fixture writers are reused. No parser, sampler, matrix
product, guessed controller clock or retail animation expectation is copied.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import check_set as writer
import check_pose

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def bits(time): return struct.unpack('<Q',struct.pack('<d',time))[0]

CASES = [
 ('first',-2,-2,[[0,2,0,-3],[-2,0,0,12],[0,0,2,-2]],[[-2,0,0,-3],[0,-2,0,16],[0,0,-2,6]]),
 ('zero',0,0,[[0,4,0,-7],[-4,0,0,4],[0,0,4,6]],[[0,0,0,1],[0,0,0,4],[0,0,0,14]]),
 ('last',2,2,[[0,6,0,-11],[-6,0,0,-4],[0,0,6,14]],[[6,0,0,13],[0,6,0,-16],[0,0,6,14]]),
 ('mixed',0,1,[[0,4,0,-7],[-4,0,0,4],[0,0,4,6]],[[2,0,0,5],[0,2,0,0],[0,0,2,10]]),
 ('reflected-child',2,-2,[[0,6,0,-11],[-6,0,0,-4],[0,0,6,14]],[[-6,0,0,-11],[0,-6,0,8],[0,0,-6,38]]),
 ('negative-zero',-0.0,0,[[0,4,0,-7],[-4,0,0,4],[0,0,4,6]],[[0,0,0,1],[0,0,0,4],[0,0,0,14]]),
 ('mixed-repeat',0,1,[[0,4,0,-7],[-4,0,0,4],[0,0,4,6]],[[2,0,0,5],[0,2,0,0],[0,0,2,10]]),
 ('first-again',-2,-2,[[0,2,0,-3],[-2,0,0,12],[0,0,2,-2]],[[-2,0,0,-3],[0,-2,0,16],[0,0,-2,6]]),
]

def check(binary, prior_binary, output):
    binary=binary.resolve();prior_binary=prior_binary.resolve();output=output.resolve()
    output.mkdir(parents=True,exist_ok=False)
    inputs=output/'inputs';inputs.mkdir();requests=output/'requests';requests.mkdir()
    blocks=writer.fixture();blocks[0],blocks[2]=blocks[2],blocks[0]
    blocks[1]=('NiNode',check_pose.node(3,[0],[100,101,102],writer.R90,99))
    blocks[6]=('NiTransformController',writer.controller(0,7))
    source=inputs/'authored.nif';source.write_bytes(writer.container(blocks,root=2))
    digest=list(bytes.fromhex(sha(source)))
    bindings=[dict(object=1,controller=3),dict(object=0,controller=6)]
    def rows(parent,child):
        return [dict(expected_source_sha256=digest.copy(),object=0,controller=6,source_time=child),
                dict(expected_source_sha256=digest.copy(),object=1,controller=3,source_time=parent)]
    base=dict(schema_version=1,expected_sha256=digest,bindings=bindings,
              time_vectors=[rows(parent,child) for _,parent,child,_,_ in CASES])
    frozen={str(p):sha(p) for p in [source,binary,prior_binary]};commands=[];refusals=[];equal=[]
    def write_request(name,value):
        path=requests/(name+'.json');path.write_text(json.dumps(value)+'\n');frozen[str(path)]=sha(path);return path
    def invoke(exe,name,args,error=None,destination=None):
        dest=destination or output/(name+'.json');command=[str(exe),*args,'--output',str(dest)]
        process=subprocess.run(command,capture_output=True,text=True)
        (output/(name+'.stderr.txt')).write_text(process.stderr)
        assert process.returncode==int(error is not None),(name,process.stderr)
        if error in ('schema','count','protected'):
            assert not dest.exists(),name
            required={'schema':('unknown field','missing field','invalid type'),
                      'count':('requires schema1',),'protected':('outside every source directory',)}[error]
            assert any(value in process.stderr for value in required),(name,process.stderr)
            value=None
        else:
            value=json.loads(dest.read_text());assert value['failures']==int(error is not None),value
            if error:assert value['evaluation'] is None and error in value['error'],value
            else:assert value['evaluation'] is not None and value['sha256']==sha(source),value
        commands.append(dict(command=command,exit_code=process.returncode,intended_error=error,receipt_sha256=sha(dest) if dest.exists() else None))
        return value,dest
    def run(name,request,error=None,input_path=source,destination=None):
        report,_=invoke(binary,name,['nif-source-pose-set',str(input_path),'--prepared','--request',str(write_request(name,request))],error,destination)
        if error:refusals.append(name);return None
        return report['evaluation']
    value=run('ordered',base)
    usage=value['preparation'];source_usage=usage['source_preparation']
    assert (usage['required_forest_constructions'],usage['required_objects'],usage['admitted_channels'])==(1,3,2)
    assert (source_usage['animation_key_decodes'],source_usage['scene_decodes'],source_usage['map_constructions'],source_usage['source_sha256_computations'],source_usage['block_sha256_computations'])==(1,1,1,1,9)
    assert usage['live_retained_bytes']==usage['source_storage_bytes']+usage['extra_retained_bytes']
    assert usage['source_storage_bytes']>=source_usage['scene_array_admission_bytes']+source_usage['extra_retained_bytes']+source_usage['animation_key_retained_bytes']
    assert value['sample_work']['validation_units']==usage['sample_work']['validation_units']+sum(v['sample_work']['validation_units'] for v in value['samples'])
    assert all(v['sample_work']['validation_units']==28 for v in value['samples'])
    assert not value['retail_behavior_verified']
    for (name,parent,child,parent_world,child_world),sample in zip(CASES,value['samples']):
        assert sample['propagated_objects']==3 and sample['preparation']==source_usage
        assert [o['channel']['object']['block'] for o in sample['objects']]==[1,0]
        assert [o['channel']['requested_time_f64_bits'] for o in sample['objects']]==[bits(parent),bits(child)]
        assert [o['source_world'] for o in sample['objects']]==[parent_world,child_world],name
        assert [(a['source']['block'],a['applied_object']) for a in sample['objects'][1]['ancestors']]==[(1,1),(2,None)]
        assert sample['objects'][1]['ancestors'][0]['effective_local']==sample['objects'][0]['channel']['local']
        old_request=dict(schema_version=1,expected_sha256=digest,requests=[dict(object=1,controller=3,source_time=parent),dict(object=0,controller=6,source_time=child)])
        path=write_request('single-'+name,old_request)
        args=['nif-source-pose-set',str(source),'--request',str(path)]
        old,old_path=invoke(prior_binary,'prior-'+name,args)
        current,current_path=invoke(binary,'current-'+name,args)
        assert old_path.read_bytes()==current_path.read_bytes(),name;equal.append(name)
        assert sample['objects']==old['evaluation']['objects'],name
        for object_ in sample['objects']:
            channel=object_['channel']
            for span in [channel[key] for key in ['object','controller','interpolator','data']]+[ancestor['source'] for ancestor in object_['ancestors']]:
                packet=blocks[span['block']][1]
                offset=len(source.read_bytes())-8-sum(len(p) for _,p in blocks[span['block']:])
                assert (span['offset'],span['bytes'],span['sha256'])==(offset,len(packet),hashlib.sha256(packet).hexdigest()),span
    reverse=copy.deepcopy(base);reverse['bindings'].reverse()
    permuted=run('binding-order',reverse)
    assert permuted['preparation']==usage
    assert (permuted['retained_bytes'],permuted['work_units'],permuted['sample_work'])==(value['retained_bytes'],value['work_units'],value['sample_work'])
    for first,second in zip(value['samples'],permuted['samples']):assert first['objects']==second['objects'][::-1]
    reverse=copy.deepcopy(base)
    for vector in reverse['time_vectors']:vector.reverse()
    assert run('time-row-order',reverse)==value
    double=copy.deepcopy(base);double['time_vectors']+=copy.deepcopy(base['time_vectors'])
    twice=run('twice-ordered',double)
    assert twice['preparation']==usage and twice['samples']==value['samples']*2
    assert twice['sample_work']['validation_units']-value['sample_work']['validation_units']==8*28
    assert twice['work_units']-value['work_units']==sum(s['work_units'] for s in value['samples'])
    for name,change,error in [
        ('stale-global',lambda r:r['expected_sha256'].__setitem__(0,r['expected_sha256'][0]^1),'SHA256 differs'),
        ('stale-last-time',lambda r:r['time_vectors'][-1][-1]['expected_source_sha256'].__setitem__(0,r['time_vectors'][-1][-1]['expected_source_sha256'][0]^1),'time source SHA256 differs'),
        ('missing-parent',lambda r:r['bindings'].pop(0),'required ancestor 1 controller 3 is not explicitly selected'),
        ('duplicate-binding',lambda r:r['bindings'].__setitem__(1,dict(r['bindings'][0])),'duplicate prepared set object'),
        ('wrong-binding',lambda r:r['bindings'][1].__setitem__('controller',3),'object.controller differs'),
        ('missing-time',lambda r:r['time_vectors'][-1].pop(),'missing or excess'),
        ('duplicate-time',lambda r:r['time_vectors'][-1].__setitem__(1,dict(r['time_vectors'][-1][0])),'duplicate prepared set time object'),
        ('foreign-time',lambda r:r['time_vectors'][-1][1].__setitem__('object',4294967295),'foreign prepared set time object'),
        ('wrong-time-controller',lambda r:r['time_vectors'][-1][1].__setitem__('controller',6),'time controller differs'),
        ('late-time',lambda r:r['time_vectors'][-1][1].__setitem__('source_time',3),'extrapolate'),
    ]:
        request=copy.deepcopy(base);change(request);run(name,request,error)
    rotated=copy.deepcopy(blocks);rotated[8]=('NiTransformData',writer.fixtures.w(1,1)+writer.fixtures.f(0,1,0,0,0)+rotated[8][1][4:])
    bad_source=inputs/'rotation.nif';bad_source.write_bytes(writer.container(rotated,root=2));frozen[str(bad_source)]=sha(bad_source)
    changed=copy.deepcopy(base);changed['expected_sha256']=list(bytes.fromhex(sha(bad_source)))
    run('unsupported-rotation',changed,'rotation key mapping is unapplied',bad_source)
    for name,change in [
        ('top-extra',lambda r:r.__setitem__('extra',1)),
        ('binding-extra',lambda r:r['bindings'][0].__setitem__('extra',1)),
        ('time-extra',lambda r:r['time_vectors'][0][0].__setitem__('extra',1)),
        ('missing-time-sha',lambda r:r['time_vectors'][0][0].pop('expected_source_sha256')),
        ('missing-bind-controller',lambda r:r['bindings'][0].pop('controller')),
        ('missing-time-object',lambda r:r['time_vectors'][0][0].pop('object')),
        ('missing-time-value',lambda r:r['time_vectors'][0][0].pop('source_time')),
        ('null-vector',lambda r:r['time_vectors'].__setitem__(0,None)),
    ]:
        request=copy.deepcopy(base);change(request);run(name,request,'schema')
    for name,change in [
        ('schema',lambda r:r.__setitem__('schema_version',2)),
        ('empty-bindings',lambda r:r.__setitem__('bindings',[])),
        ('many-bindings',lambda r:r.__setitem__('bindings',[r['bindings'][0]]*257)),
        ('empty-times',lambda r:r.__setitem__('time_vectors',[])),
        ('many-vectors',lambda r:r.__setitem__('time_vectors',[[r['time_vectors'][0][0]]]*65)),
        ('many-time-rows',lambda r:r.__setitem__('time_vectors',[[r['time_vectors'][0][0]]*257])),
    ]:
        request=copy.deepcopy(base);change(request);run(name,request,'count')
    run('protected-source',base,'protected',destination=inputs/'blocked.json')
    run('protected-request',base,'protected',destination=requests/'blocked.json')
    assert run('after-refusals',base)==value
    for path,digest in frozen.items():assert sha(Path(path))==digest,path
    result=dict(contract='engineering-prepared-source-pose-set-batch-v1',literal_time_vectors=[row[0] for row in CASES],
                prior_complete_reports_byte_equal=equal,semantic_schema_and_guard_refusals=refusals,
                commands=commands,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--prior-binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();result=check(args.binary,args.prior_binary,args.output)
    print(json.dumps({key:value for key,value in result.items() if key not in ('commands','frozen_hashes')}))
