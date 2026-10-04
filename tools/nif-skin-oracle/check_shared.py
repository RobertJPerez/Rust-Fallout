"""Independent shared-root source and literal expectations through the real CLI."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess

import check_sampled as writer

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def fixture():
    blocks = writer.fixture()
    blocks[0] = ('NiNode', writer.node(writer.R90, [4, -2, 8], 2, [1, 2, 3, 10]))
    geometry = writer.av(writer.RM90, [1000, -2000, 3000], 77) + writer.words(13, 11, 0, writer.NULL) + bytes([0])
    skin = writer.transform(writer.R90, [-3, 2, -1], 1) + writer.words(2) + bytes([1])
    for _ in range(2):
        skin += writer.transform(writer.ID, [0, 0, 0], 1) + writer.floats(0, 0, 0, 10) + writer.shorts(3)
        for vertex in range(3):
            skin += writer.shorts(vertex) + writer.floats(.5)
    blocks.extend([('NiTriShape', geometry), ('NiSkinInstance', writer.words(12, writer.NULL, 0, 2, 2, 1)),
                   ('NiSkinData', skin), ('NiTriShapeData', blocks[6][1])])
    return writer.container(blocks)

def check(binary, output, previous_binary=None):
    binary = binary.resolve(); output.mkdir(parents=True, exist_ok=False)
    inputs=output/'inputs';inputs.mkdir();requests=output/'requests';requests.mkdir()
    source=inputs/'shared.nif';source.write_bytes(fixture())
    frozen={str(path):sha(path) for path in [binary,source]}
    if previous_binary is not None:
        previous_binary=previous_binary.resolve();frozen[str(previous_binary)]=sha(previous_binary)
    base=dict(schema_version=1,expected_source_sha256=list(bytes.fromhex(sha(source))),geometries=[
        dict(geometry=3,weights=dict(kind='require_unit_sum',absolute_tolerance=0)),
        dict(geometry=10,weights=dict(kind='require_unit_sum',absolute_tolerance=0))])
    invocations=[]
    def run(name,request,error=None,destination=None,extra=()):
        path=requests/(name+'.json');path.write_text(json.dumps(request)+'\n');frozen[str(path)]=sha(path)
        destination=destination or output/(name+'.json')
        command=[str(binary),'nif-skin',str(source),'--shared-skin-request',str(path),*extra,'--output',str(destination)]
        process=subprocess.run(command,capture_output=True,text=True)
        if error in ('schema','protected','conflict'):
            assert process.returncode!=0 and not destination.exists(),process.stdout+process.stderr
            value=None
        else:
            report=json.loads(destination.read_text());assert process.returncode==(0 if error is None else 1),process.stderr
            if error:
                assert report['evaluation'] is None and error in report['error'],report;value=None
            else:
                value=report['evaluation'];assert value['source_sha256']==sha(source) and not value['retail_behavior_verified']
                assert all(p['source_sha256']==sha(source) and not p['retail_behavior_verified'] for p in value['geometries'])
        invocations.append(dict(command=command,exit_code=process.returncode,expected_error=error,
                                receipt_sha256=sha(destination) if destination.exists() else None))
        return value
    value=run('normal',base)
    first,second=value['geometries']
    assert (first['geometry'],first['geometry_data'],first['instance'],first['skin_data'],first['skeleton_root'])==(3,6,4,5,0)
    assert (second['geometry'],second['geometry_data'],second['instance'],second['skin_data'],second['skeleton_root'])==(10,13,11,12,0)
    assert [(p['ordinal'],p['node']) for p in first['palette']]==[(0,1),(1,2)]
    assert [(p['ordinal'],p['node']) for p in second['palette']]==[(0,2),(1,1)]
    assert first['palette'][0]['matrix']==[[0,1,0,2.5],[-1,0,0,-.5],[0,0,1,5]]
    assert first['palette'][1]['matrix']==[[0,-3,0,0],[3,0,0,-1.5],[0,0,3,3.5]]
    assert first['positions']==[[2.625,2.75,11.375],[6.5,1.5,6],[-9,-1.5,.5]]
    assert first['normals']==[[-4,2,0],[2,-1,0],[-6,3,0]]
    assert first['skin_to_source_world']==[[0,-4,0,-4],[4,0,0,-6],[0,0,4,-4]]
    assert second['positions']==[[-2,4.5,7],[-13.5,3.5,2],[-10,5.5,-3]]
    assert second['normals']==[[-2,3.5,0]]*3
    assert second['skin_to_source_world']==[[2,0,0,10],[0,2,0,-6],[0,0,2,10]]
    assert second['palette'][0]['matrix']==[[0,-3,0,-7],[3,0,0,0],[0,0,3,0]]
    assert second['palette'][1]['matrix']==[[2,0,0,-4],[0,2,0,5],[0,0,2,-1]]
    assert first['weight_sums']==second['weight_sums']==[1,1,1]
    assert first['unapplied_controllers']==second['unapplied_controllers']==[dict(object=1,controller=7)]
    reversed_request=copy.deepcopy(base);reversed_request['geometries'].reverse()
    reverse=run('reversed',reversed_request)
    assert reverse['geometries']==value['geometries'][::-1]
    assert (value['retained_bytes'],value['work_units'])==(reverse['retained_bytes'],reverse['work_units'])
    raw=copy.deepcopy(base);raw['geometries'][1]['weights']=dict(kind='preserve_raw_nonnegative')
    raw_value=run('raw-policy',raw);assert raw_value['geometries'][1]['positions']==second['positions']
    prior_exact=[]
    if previous_binary is not None:
        for request,expected in zip(base['geometries'],value['geometries']):
            destination=output/('prior-'+str(request['geometry'])+'.json')
            command=[str(previous_binary),'nif-skin',str(source),'--pose-geometry',str(request['geometry']),'--pose-weight-tolerance','0','--output',str(destination)]
            process=subprocess.run(command,capture_output=True,text=True);assert process.returncode==0,process.stderr
            report=json.loads(destination.read_text());assert report['evaluation']==expected
            invocations.append(dict(command=command,exit_code=0,expected_error=None,receipt_sha256=sha(destination)))
            prior_exact.append(request['geometry'])
    wrong=copy.deepcopy(base);wrong['expected_source_sha256'][0]^=1;run('stale',wrong,'SHA256 differs')
    run('duplicate',dict(base,geometries=[base['geometries'][0]]*2),'duplicate geometry')
    run('empty',dict(base,geometries=[]),'nonempty')
    bad=copy.deepcopy(base);bad['geometries'][1]['geometry']=999;run('later-geometry',bad,'selected geometry has no decoded skin owner')
    bad=copy.deepcopy(base);bad['geometries'][1]['weights']['absolute_tolerance']=-1;run('later-policy',bad,'weight tolerance')
    run('schema',dict(base,schema_version=2),'schema');run('extra',dict(base,extra=True),'schema')
    run('count',dict(base,geometries=base['geometries']*33),'schema')
    bad=copy.deepcopy(base);bad['geometries'][1]['weights']=dict(kind='normalize');run('unknown-policy',bad,'schema')
    run('conflicting-mode',base.copy(),'conflict',extra=['--include-bindings'])
    for label,folder in [('input',inputs),('request',requests)]:
        run('protected-'+label,base.copy(),'protected',destination=folder/'blocked.json')
    for path,digest in frozen.items():assert sha(Path(path))==digest
    result=dict(contract='engineering-shared-source-skin-batch-v1',literal_geometries=2,full_permutation_cases=1,
                prior_single_geometry_objects_exact=prior_exact,intended_refusals=sum(p['expected_error'] is not None for p in invocations),
                invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--previous-binary',type=Path)
    args=parser.parse_args();result=check(args.binary,args.output,args.previous_binary)
    print(json.dumps({key:value for key,value in result.items() if key not in ('invocations','frozen_hashes')}))
