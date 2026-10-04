"""Independent sparse raw-weight source consumed by the frozen skin CLI."""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import check_sampled as source_writer

def fixture():
    blocks=source_writer.fixture()
    skin=source_writer.transform(source_writer.ID,[1,-2,3],.5)+source_writer.words(2)+bytes([1])
    for rotation,translation,scale,influences in [
        (source_writer.ID,[-1,0,2],1,[(2,.125),(0,.5),(0,.25),(1,1),(0,-0.),(0,.125),(0,.125),(0,0)]),
        (source_writer.R90,[0,-1,0],2,[(0,.5),(2,.875)]),
    ]:
        skin+=source_writer.transform(rotation,translation,scale)+source_writer.floats(0,0,0,10)+source_writer.shorts(len(influences))
        for vertex,weight in influences:skin+=source_writer.shorts(vertex)+source_writer.floats(weight)
    blocks[5]=('NiSkinData',skin)
    return blocks

def check(binary,output):
    binary=binary.resolve();output.mkdir(parents=True,exist_ok=False)
    inputs=output/'inputs';inputs.mkdir();requests=output/'requests';requests.mkdir()
    source=inputs/'authored.nif';source.write_bytes(source_writer.container(fixture()))
    frozen={str(path):source_writer.sha(path) for path in [source,binary]}
    base=dict(schema_version=1,expected_sha256=list(bytes.fromhex(source_writer.sha(source))),geometry=3,weights=dict(kind='preserve_raw_nonnegative'))
    invocations=[]
    def run(name,request,error=None,destination=None):
        path=requests/(name+'.json');path.write_text(json.dumps(request)+'\n');frozen[str(path)]=source_writer.sha(path)
        destination=destination or output/(name+'.json')
        command=[str(binary),'nif-skin',str(source),'--influences-request',str(path),'--output',str(destination)]
        process=subprocess.run(command,capture_output=True,text=True)
        if error in ('schema','protected'):
            assert process.returncode!=0 and not destination.exists(),process.stdout+process.stderr
        else:
            report=json.loads(destination.read_text());assert process.returncode==(0 if error is None else 1),process.stderr
            if error:assert report['evaluation'] is None and report['failures']==1 and error in report['error'],report
            else:
                value=report['evaluation'];table=value['table'];pose=value['pose']
                assert table['source_sha256']==pose['source_sha256']==source_writer.sha(source)
                assert table['vertex_offsets']==[0,7,8,10]
                expected=[(0,1,0x3f000000),(0,2,0x3e800000),(0,4,0x80000000),(0,5,0x3e000000),(0,6,0x3e000000),(0,7,0),(1,0,0x3f000000),(0,3,0x3f800000),(0,0,0x3e000000),(1,1,0x3f600000)]
                assert [(e['bone_ordinal'],e['source_weight_ordinal'],e['weight_bits']) for e in table['entries']]==expected
                assert pose['positions']==[[3,-.25,14.25],[6.5,1.5,6],[-7.1875,-1.375,.9375]],pose['positions']
                assert pose['normals']==[[-1,.5,0],[2,-1,0],[-5,2.5,0]],pose['normals']
                assert pose['weight_sums']==[1.5,1,1]
                assert not table['retail_behavior_verified'] and not pose['retail_behavior_verified']
        invocations.append(dict(command=command,exit_code=process.returncode,expected_error=error,receipt_sha256=source_writer.sha(destination) if destination.exists() else None))
        return None if error else value
    value=run('raw',copy.deepcopy(base))
    stale=base['expected_sha256'].copy();stale[0]^=1;run('stale',dict(base,expected_sha256=stale),'source SHA256 differs')
    run('geometry',dict(base,geometry=1),'selected geometry has no decoded skin owner')
    run('unit-sum',dict(base,weights=dict(kind='require_unit_sum',absolute_tolerance=0)),'raw weight sum 1.5')
    run('tolerance',dict(base,weights=dict(kind='require_unit_sum',absolute_tolerance=-1)),'weight tolerance must be finite')
    run('schema',dict(base,schema_version=2),'schema');run('extra',dict(base,extra=1),'schema')
    run('policy',dict(base,weights=dict(kind='four_weights')),'schema')
    run('protected-request',base.copy(),'protected',destination=requests/'blocked-output.json')
    run('protected-source',base.copy(),'protected',destination=inputs/'blocked-output.json')
    for path,digest in frozen.items():assert source_writer.sha(Path(path))==digest
    summary=dict(contract='engineering-exact-raw-influence-csr-v1',analytic_cases=1,intended_refusals=9,
        usage=value['table']['usage'],invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');return summary

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    arguments=parser.parse_args();result=check(arguments.binary,arguments.output)
    print(json.dumps({key:value for key,value in result.items() if key not in ('invocations','frozen_hashes')}))
