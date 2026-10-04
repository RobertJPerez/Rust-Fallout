"""Independent literal subset coordinates; original gameplay remains unmeasured."""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import check_sampled as writer

sha=writer.sha
def packet(mapping,strip=False,palette=(1,0)):
    raw=writer.shorts(len(mapping),1,len(palette),int(strip),4,*palette)+bytes([1])+writer.shorts(*mapping)+bytes([1])
    for _ in mapping:raw+=writer.floats(.5,0,.5,0)
    if strip:raw+=writer.shorts(4)
    raw+=bytes([1])+writer.shorts(*( [1,0,0,1] if strip else [2,0,1]))+bytes([1])
    return raw+bytes([0,1,0,1]*len(mapping))
def fixture(mapping=(2,0,2),palette=(1,0),nonunit=False,body_count=2):
    blocks=writer.fixture()
    blocks[4]=('BSDismemberSkinInstance',writer.words(5,10,0,2,1,2,body_count)+writer.shorts(*([257,7000,1,10][:body_count*2])))
    if nonunit:
        skin=bytearray(blocks[5][1]);skin[129:133]=writer.floats(.5);blocks[5]=('NiSkinData',bytes(skin))
    blocks.append(('NiSkinPartition',writer.words(2)+packet(mapping,palette=palette)+packet((1,0),True)))
    raw=writer.container(blocks);offset=len(raw)-sum(len(data) for _,data in blocks)-8
    spans={}
    for i,(_,data) in enumerate(blocks):
        spans[i]=dict(block=i,offset=offset,bytes=len(data),sha256=__import__('hashlib').sha256(data).hexdigest());offset+=len(data)
    return raw,spans

def check(binary,output,previous_binary=None):
    binary=binary.resolve();output.mkdir(parents=True,exist_ok=False)
    inputs=output/'inputs';inputs.mkdir();requests=output/'requests';requests.mkdir()
    source=inputs/'authored.nif';raw,spans=fixture();source.write_bytes(raw)
    frozen={str(p):sha(p) for p in [binary,source]};invocations=[]
    base=dict(schema_version=1,expected_source_sha256=list(bytes.fromhex(sha(source))),geometry=3,partition_block=10,partition_ordinal=0,weights=dict(kind='require_unit_sum',absolute_tolerance=0))
    def run(name,request,error=None,input_path=source,destination=None,extra=()):
        path=requests/(name+'.json');path.write_text(json.dumps(request)+'\n');frozen[str(path)]=sha(path)
        destination=destination or output/(name+'.json')
        command=[str(binary),'nif-skin',str(input_path),'--partition-pose-request',str(path),*extra,'--output',str(destination)]
        p=subprocess.run(command,capture_output=True,text=True)
        if error in ('schema','protected','conflict'):
            assert p.returncode!=0 and not destination.exists(),p.stdout+p.stderr;value=None
        else:
            report=json.loads(destination.read_text());assert p.returncode==(0 if error is None else 1),p.stderr
            if error:assert report['evaluation'] is None and error in report['error'],report;value=None
            else:value=report['evaluation'];assert value['source_sha256']==sha(input_path) and not value['retail_behavior_verified']
        invocations.append(dict(command=command,exit_code=p.returncode,expected_error=error,receipt_sha256=sha(destination) if destination.exists() else None));return value
    value=run('triangles',base)
    assert (value['geometry'],value['geometry_data'],value['instance'],value['skin_data'],value['skeleton_root'],value['source_vertex_count'])==(3,6,4,5,0,3)
    assert value['partition']==dict(spans[10],ordinal=0)
    assert value['vertices']==[dict(partition_vertex=0,source_vertex=2,position=[-9,-1.5,.5],normal=[-6,3,0],weight_sum=1),
                               dict(partition_vertex=1,source_vertex=0,position=[2.625,2.75,11.375],normal=[-4,2,0],weight_sum=1),
                               dict(partition_vertex=2,source_vertex=2,position=[-9,-1.5,.5],normal=[-6,3,0],weight_sum=1)]
    assert value['palette'][0]['matrix']==[[0,1,0,2.5],[-1,0,0,-.5],[0,0,1,5]]
    assert value['palette'][1]['matrix']==[[0,-3,0,0],[3,0,0,-1.5],[0,0,3,3.5]]
    assert value['skin_to_source_world']==[[0,-4,0,-4],[4,0,0,-6],[0,0,4,-4]]
    assert value['source_to_partition_offsets']==[0,1,1,3] and value['source_to_partition_vertices']==[1,0,2]
    assert value['topology']==dict(kind='triangles',triangles=[[2,0,1]]) and value['body_part']==dict(flags=257,body_part=7000)
    assert value['unapplied_controllers']==[dict(object=1,controller=7)]
    assert value['partition_palette']==[dict(local_bone=0,global_bone_ordinal=1,source_bone_node=2),dict(local_bone=1,global_bone_ordinal=0,source_bone_node=1)]
    assert [value['usage'][k] for k in ('binding_decodes','scene_decodes','geometry_deformations')]==[1,1,1]
    assert value['usage']['retained_bytes']==value['usage']['subset_retained_bytes']+value['usage']['full_pose_retained_bytes']
    strip=run('strips',dict(base,partition_ordinal=1))
    assert strip['vertices']==[dict(partition_vertex=0,source_vertex=1,position=[6.5,1.5,6],normal=[2,-1,0],weight_sum=1),
                               dict(partition_vertex=1,source_vertex=0,position=[2.625,2.75,11.375],normal=[-4,2,0],weight_sum=1)]
    assert strip['source_to_partition_offsets']==[0,1,2,2] and strip['source_to_partition_vertices']==[1,0]
    assert strip['topology']==dict(kind='strips',lengths=[4],strips=[[1,0,0,1]]) and strip['body_part']==dict(flags=1,body_part=10)
    assert strip['declared_triangles']==1 and strip['palette']==value['palette']
    raw_source=inputs/'nonunit.nif';raw_source.write_bytes(fixture(nonunit=True)[0]);frozen[str(raw_source)]=sha(raw_source)
    raw_request=dict(base,expected_source_sha256=list(bytes.fromhex(sha(raw_source))))
    run('nonunit-refusal',raw_request,'raw weight sum 1.25',input_path=raw_source)
    raw_value=run('raw-policy',dict(raw_request,weights=dict(kind='preserve_raw_nonnegative')),input_path=raw_source)
    assert raw_value['vertices'][1]==dict(partition_vertex=1,source_vertex=0,position=[3,2.125,13.375],normal=[-3.5,1.75,0],weight_sum=1.25)
    wrong=copy.deepcopy(base);wrong['expected_source_sha256'][0]^=1;run('stale',wrong,'SHA256 differs')
    run('geometry',dict(base,geometry=0),'no decoded skin owner');run('partition',dict(base,partition_block=5),'partition link differs')
    run('ordinal',dict(base,partition_ordinal=2),'ordinal unavailable')
    run('weight-policy',dict(base,weights=dict(kind='require_unit_sum',absolute_tolerance=-1)),'weight tolerance')
    for label,args,error in [('map',dict(mapping=(3,0,2)),'outside linked geometry'),('palette',dict(palette=(2,0)),'outside linked instance'),('body-count',dict(body_count=1),'body-part association unavailable')]:
        bad=inputs/(label+'.nif');bad.write_bytes(fixture(**args)[0]);frozen[str(bad)]=sha(bad)
        run(label,dict(base,expected_source_sha256=list(bytes.fromhex(sha(bad)))),error,input_path=bad)
    run('schema',dict(base,schema_version=2),'schema');run('extra',dict(base,extra=True),'schema')
    run('policy-schema',dict(base,weights=dict(kind='normalize')),'schema')
    run('conflict',base.copy(),'conflict',extra=['--include-partitions'])
    for label,folder in [('input',inputs),('request',requests)]:run('protected-'+label,base.copy(),'protected',destination=folder/'blocked.json')
    prior=[]
    if previous_binary is not None:
        previous_binary=previous_binary.resolve();frozen[str(previous_binary)]=sha(previous_binary)
        for name,flags in [('schema1',[]),('schema2',['--include-partitions']),('schema3',['--include-bindings']),('stored-pose',['--pose-geometry','3','--pose-weight-tolerance','0'])]:
            receipts=[]
            for label,exe in [('old',previous_binary),('new',binary)]:
                path=output/(label+'-'+name+'.json');command=[str(exe),'nif-skin',str(source),*flags,'--output',str(path)]
                p=subprocess.run(command,capture_output=True,text=True);assert p.returncode==0,p.stderr;receipts.append(path.read_bytes())
                invocations.append(dict(command=command,exit_code=0,receipt_sha256=sha(path)))
            assert receipts[0]==receipts[1];prior.append(name)
    for path,digest in frozen.items():assert sha(Path(path))==digest
    result=dict(contract='engineering-authored-partition-subset-proof-v1',literal_subsets=3,literal_vertices=8,intended_refusals=sum(p.get('expected_error') is not None for p in invocations),
                prior_receipts_byte_equal=prior,invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--previous-binary',type=Path)
    args=parser.parse_args();result=check(args.binary,args.output,args.previous_binary);print(json.dumps({k:v for k,v in result.items() if k not in ('invocations','frozen_hashes')}))
