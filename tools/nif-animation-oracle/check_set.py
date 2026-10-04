"""Second three-node source with literal simultaneous-channel expectations."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import fixtures
import check_pose

NULL=0xffffffff
R90=[0,-1,0,1,0,0,0,0,1]
R180=[-1,0,0,0,-1,0,0,0,1]

def keys(first,last,first_scale,last_scale):
    return fixtures.w(0,2,1)+fixtures.f(-2,*first,2,*last)+fixtures.w(2,1)+fixtures.f(-2,first_scale,2,last_scale)

def controller(target,interpolator):
    return fixtures.w(NULL)+fixtures.h(0xffff)+fixtures.f(17,-9,100,101)+fixtures.w(target,interpolator)

def interpolator(data):return fixtures.f(1000,2000,3000,2,-3,4,-5,12)+fixtures.w(data)

def fixture():
    return [
        ('NiNode',check_pose.node(NULL,[1],[1,4,-2],R180,2)),
        ('NiNode',check_pose.node(3,[2],[100,101,102],R90,99)),
        ('NiNode',check_pose.node(6,[],[404,405,406],R90,88)),
        ('NiTransformController',controller(1,4)),('NiTransformInterpolator',interpolator(5)),
        ('NiTransformData',keys([2,-4,0],[6,4,8],1,3)),
        ('NiTransformController',controller(2,7)),('NiTransformInterpolator',interpolator(8)),
        ('NiTransformData',keys([-2,0,4],[2,4,0],-1,1)),
    ]

def container(blocks):return fixtures.container(34,blocks,[])[:-8]+fixtures.w(1,0)
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()

def check(binary,output):
    binary=binary.resolve();output.mkdir(parents=True,exist_ok=False)
    inputs=output/'inputs';inputs.mkdir();requests=output/'requests';requests.mkdir()
    source=inputs/'authored.nif';source.write_bytes(container(fixture()))
    frozen={str(path):sha(path) for path in [source,binary]}
    base=dict(schema_version=1,expected_sha256=list(bytes.fromhex(sha(source))),requests=[dict(object=1,controller=3,source_time=1),dict(object=2,controller=6,source_time=1)])
    invocations=[]
    def run(name,request,error=None,input_path=source,destination=None):
        path=requests/(name+'.json');path.write_text(json.dumps(request)+'\n');frozen[str(path)]=sha(path)
        destination=destination or output/(name+'.json')
        command=[str(binary),'nif-source-pose-set',str(input_path),'--request',str(path),'--output',str(destination)]
        process=subprocess.run(command,capture_output=True,text=True)
        if error in ('schema','protected','count'):
            assert process.returncode!=0 and not destination.exists(),process.stdout+process.stderr
        else:
            report=json.loads(destination.read_text());assert process.returncode==(0 if error is None else 1),process.stderr
            if error:
                assert report['evaluation'] is None and report['failures']==1 and error in report['error'],report
            else:
                value=report['evaluation'];assert value['source_sha256']==sha(input_path) and not value['retail_behavior_verified']
                assert [item['channel']['object']['block'] for item in value['objects']]==[entry['object'] for entry in request['requests']]
                assert [item['channel']['requested_time_f64_bits'] for item in value['objects']]==[struct.unpack('<Q',struct.pack('<d',entry['source_time']))[0] for entry in request['requests']]
        invocations.append(dict(command=command,exit_code=process.returncode,expected_error=error,receipt_sha256=sha(destination) if destination.exists() else None))
        return None if error else value
    cases=[
        ('first',-2,[[0,2,0,-3],[-2,0,0,12],[0,0,2,-2]],[[-2,0,0,-3],[0,-2,0,16],[0,0,-2,6]]),
        ('zero',0,[[0,4,0,-7],[-4,0,0,4],[0,0,4,6]],[[0,0,0,1],[0,0,0,4],[0,0,0,14]]),
        ('positive',1,[[0,5,0,-9],[-5,0,0,0],[0,0,5,10]],[[2.5,0,0,6],[0,2.5,0,-5],[0,0,2.5,15]]),
        ('last',2,[[0,6,0,-11],[-6,0,0,-4],[0,0,6,14]],[[6,0,0,13],[0,6,0,-16],[0,0,6,14]]),
    ]
    positive=None
    for name,time,parent,child in cases:
        request=copy.deepcopy(base)
        for entry in request['requests']:entry['source_time']=time
        value=run(name,request)
        assert value['objects'][0]['source_world']==parent,(name,value['objects'][0]['source_world'])
        assert value['objects'][1]['source_world']==child,(name,value['objects'][1]['source_world'])
        assert value['propagated_objects']==3
        if name=='positive':positive=value
    permuted=run('permuted',dict(base,requests=base['requests'][::-1]))
    assert positive['objects']==permuted['objects'][::-1]
    mixed=copy.deepcopy(base);mixed['requests'][0]['source_time']=0
    value=run('mixed',mixed);assert value['objects'][1]['source_world']==[[2,0,0,5],[0,2,0,0],[0,0,2,10]]
    stale=base['expected_sha256'].copy();stale[0]^=1
    run('stale',dict(base,expected_sha256=stale),'SHA256 differs')
    run('duplicate',dict(base,requests=[base['requests'][0],base['requests'][0]]),'duplicate pose set object 1')
    run('missing-parent',dict(base,requests=[base['requests'][1]]),'required ancestor 1 controller 3 is not explicitly selected')
    bad=copy.deepcopy(base);bad['requests'][1]['controller']=7;run('bad-link',bad,'object.controller differs')
    bad=copy.deepcopy(base);bad['requests'][1]['source_time']=3;run('later-range',bad,'extrapolate')
    changed=fixture();changed[8]=('NiTransformData',fixtures.w(1,1)+fixtures.f(0,1,0,0,0)+changed[8][1][4:])
    rotation=inputs/'rotation.nif';rotation.write_bytes(container(changed));frozen[str(rotation)]=sha(rotation)
    run('rotation',dict(base,expected_sha256=list(bytes.fromhex(sha(rotation)))),'rotation key mapping is unapplied',input_path=rotation)
    run('extra',dict(base,extra=1),'schema');run('schema',dict(base,schema_version=2),'schema')
    run('count',dict(base,requests=[base['requests'][0]]*257),'count')
    run('protected-request',base.copy(),'protected',destination=requests/'blocked-output.json')
    run('protected-source',base.copy(),'protected',destination=inputs/'blocked-output.json')
    for path,digest in frozen.items():assert sha(Path(path))==digest
    result=dict(contract='engineering-explicit-linked-pose-set-v1',analytic_cases=5,permutation_cases=1,intended_refusals=11,
                invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    arguments=parser.parse_args();result=check(arguments.binary,arguments.output)
    print(json.dumps({key:value for key,value in result.items() if key not in ('invocations','frozen_hashes')}))
