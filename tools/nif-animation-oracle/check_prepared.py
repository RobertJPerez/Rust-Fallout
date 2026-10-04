"""Frozen prepared-source consumer with independent literal pose expectations."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import check_pose

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()

def check(binary,output):
    binary=binary.resolve();output.mkdir(parents=True,exist_ok=False)
    inputs=output/'inputs';inputs.mkdir();requests=output/'requests';requests.mkdir()
    source=inputs/'authored.nif';source.write_bytes(check_pose.fixture())
    frozen={str(path):sha(path) for path in [source,binary]}
    base=dict(schema_version=1,expected_sha256=list(bytes.fromhex(sha(source))),object=1,controller=2,source_times=[3,-1,1.5,1,0,1.5])
    invocations=[]
    def run(name,request,error=None,destination=None):
        path=requests/(name+'.json');path.write_text(json.dumps(request)+'\n');frozen[str(path)]=sha(path)
        destination=destination or output/(name+'.json')
        command=[str(binary),'nif-source-pose-batch',str(source),'--request',str(path),'--output',str(destination)]
        process=subprocess.run(command,capture_output=True,text=True)
        if error in ('schema','protected','count'):
            assert process.returncode!=0 and not destination.exists(),process.stdout+process.stderr
        else:
            report=json.loads(destination.read_text())
            assert process.returncode==(0 if error is None else 1),process.stderr
            if error:
                assert report['failures']==1 and report['evaluation'] is None and error in report['error'],report
            else:
                value=report['evaluation'];assert value['source_sha256']==sha(source) and not value['retail_behavior_verified']
                assert len(value['samples'])==len(request['source_times'])
                assert [sample['requested_time_f64_bits'] for sample in value['samples']]==[struct.unpack('<Q',struct.pack('<d',time))[0] for time in request['source_times']]
                assert all(not sample['retail_behavior_verified'] for sample in value['samples'])
                assert {key:value['preparation'][key] for key in ['animation_key_decodes','scene_decodes','map_constructions','source_sha256_computations']}==dict(animation_key_decodes=1,scene_decodes=1,map_constructions=1,source_sha256_computations=1)
        invocations.append(dict(command=command,exit_code=process.returncode,expected_error=error,receipt_sha256=sha(destination) if destination.exists() else None))
        return None if error else value
    value=run('ordered',base.copy())
    expected={
        -1:([[2,0,0,4],[0,2,0,0],[0,0,-2,-2]],[[0,-4,0,2],[4,0,0,5],[0,0,-4,1]]),
        0:([[1,0,0,3],[0,1,0,2],[0,0,-1,0]],[[0,-2,0,-2],[2,0,0,3],[0,0,-2,5]]),
        1:([[0,0,0,2],[0,0,0,4],[0,0,0,2]],[[0,0,0,-6],[0,0,0,1],[0,0,0,9]]),
        1.5:([[-.5,0,0,1.5],[0,-.5,0,5],[0,0,.5,3]],[[0,1,0,-8],[-1,0,0,0],[0,0,1,11]]),
        3:([[-2,0,0,0],[0,-2,0,8],[0,0,2,6]],[[0,4,0,-14],[-4,0,0,-3],[0,0,4,17]]),
    }
    for time,sample in zip(base['source_times'],value['samples']):
        assert (sample['local'],sample['source_world'])==expected[time]
        assert sample['unapplied_controller_fields']['flags']==0xffff
    assert run('empty',dict(base,source_times=[]))['samples']==[]
    transport_time=0.23005015640173943
    transport=run('transport',dict(base,source_times=[transport_time]))
    assert transport['samples'][0]['requested_time_f64_bits']==0x3fcd72489517b330
    stale=base['expected_sha256'].copy();stale[0]^=1
    run('stale',dict(base,expected_sha256=stale),'SHA256 differs')
    run('link',dict(base,controller=3),'object.controller differs')
    run('later-failure',dict(base,source_times=[-1,1,4]),'batch request 2')
    run('before',dict(base,source_times=[-2]),'extrapolate')
    run('extra-field',dict(base,extra=1),'schema')
    run('schema',dict(base,schema_version=2),'schema')
    run('count',dict(base,source_times=[0]*65),'count')
    run('protected-request',base.copy(),'protected',destination=requests/'blocked-output.json')
    run('protected-source',base.copy(),'protected',destination=inputs/'blocked-output.json')
    for path,digest in frozen.items():assert sha(Path(path))==digest
    result=dict(contract='engineering-prepared-source-pose-batch-v1',analytic_samples=6,empty_batch=1,binary64_transport_cases=1,
                intended_refusals=9,preparation=value['preparation'],invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
    return result

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    arguments=parser.parse_args();result=check(arguments.binary,arguments.output)
    print(json.dumps({key:value for key,value in result.items() if key not in ('invocations','frozen_hashes')}))
