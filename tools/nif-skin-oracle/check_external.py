"""Independent two-file engineering mapping with literal shear/reflection math."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import check_sampled as writer

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def container(blocks,roots,strings):
    types=list(dict.fromkeys(name for name,_ in blocks));out=b'Gamebryo File Format, Version 20.2.0.7\n'+writer.words(0x14020007)+bytes([1])
    out+=writer.words(11,len(blocks),34)+bytes(3)+writer.shorts(len(types))
    for name in types:out+=writer.words(len(name))+name.encode()
    out+=writer.shorts(*(types.index(name) for name,_ in blocks))+writer.words(*(len(data) for _,data in blocks))
    out+=writer.words(len(strings),max((len(s) for s in strings),default=0))
    for name in strings:out+=writer.words(len(name))+name
    return out+writer.words(0)+b''.join(data for _,data in blocks)+writer.words(len(roots),*roots)
def fixtures():
    skin=writer.fixture()
    for node,name in [(1,0),(2,1)]:skin[node]=(skin[node][0],writer.words(name)+skin[node][1][4:])
    rig=[('NiNode',writer.node(writer.ID,[0,0,0],1,[])),
         ('NiNode',writer.node(writer.R90,[999,888,777],99,[2,4])),
         ('NiNode',writer.node([[-1,0,0],[0,-1,0],[0,0,1]],[3,-1,2],2,[3])),
         ('NiNode',writer.node(writer.R90,[-2,4,1],.5,[])),
         ('NiNode',writer.node(writer.RM90,[1,2,-3],3,[]))]
    for node,name in [(0,1),(2,0),(3,1),(4,1)]:rig[node]=(rig[node][0],writer.words(name)+rig[node][1][4:])
    return container(skin,[0],[b'Skin\xff\0',b'Twin']),rig

def check(binary,output):
    binary=binary.resolve();output.mkdir(parents=True,exist_ok=False)
    skin_dir=output/'skin-inputs';skin_dir.mkdir();rig_dir=output/'rig-inputs';rig_dir.mkdir();requests=output/'requests';requests.mkdir()
    skin=skin_dir/'authored.nif';rig=rig_dir/'authored.nif';skin_bytes,rig_blocks=fixtures();skin.write_bytes(skin_bytes)
    rig.write_bytes(container(rig_blocks,[1],[b'Rig\0',b'Twin']))
    frozen={str(path):sha(path) for path in [binary,skin,rig]}
    base=dict(schema_version=1,expected_skin_sha256=list(bytes.fromhex(sha(skin))),expected_rig_sha256=list(bytes.fromhex(sha(rig))),
        geometry=3,rig_root=1,explicit_bone_mapping=[
            dict(bone_ordinal=0,rig_node=2,expected_skin_bone_name_bytes=list(b'Skin\xff\0'),expected_rig_node_name_bytes=list(b'Rig\0')),
            dict(bone_ordinal=1,rig_node=3,expected_skin_bone_name_bytes=list(b'Twin'),expected_rig_node_name_bytes=list(b'Twin'))],
        explicit_root_space_mapping=[[1,1,0,-2],[0,-1,0,3],[0,0,2,1]],weights=dict(kind='require_unit_sum',absolute_tolerance=0))
    invocations=[]
    def run(name,request,error=None,rig_input=rig,destination=None):
        path=requests/(name+'.json');path.write_text(json.dumps(request)+'\n');frozen[str(path)]=sha(path)
        destination=destination or output/(name+'.json')
        command=[str(binary),'nif-skin',str(skin),'--external-rig',str(rig_input),'--external-skin-request',str(path),'--output',str(destination)]
        process=subprocess.run(command,capture_output=True,text=True)
        if error in ('schema','protected'):
            assert process.returncode!=0 and not destination.exists(),process.stdout+process.stderr
        else:
            report=json.loads(destination.read_text());assert process.returncode==(0 if error is None else 1),process.stderr
            if error:assert report['evaluation'] is None and report['failures']==1 and error in report['error'],report
            else:
                value=report['evaluation'];assert value['skin_source_sha256']==sha(skin) and value['rig_source_sha256']==sha(rig_input)
                assert value['explicit_root_space_mapping']==request['explicit_root_space_mapping'] and not value['retail_behavior_verified']
        invocations.append(dict(command=command,exit_code=process.returncode,expected_error=error,receipt_sha256=sha(destination) if destination.exists() else None))
        return None if error else value
    value=run('mapped',copy.deepcopy(base))
    assert value['skin']['positions']==[[-.125,3.5,14],[0,4,11.5],[1.5,1,5.5]],value['skin']['positions']
    assert value['skin']['normals']==[[1.5,-1,0],[-3,2,0],[3,-2,0]],value['skin']['normals']
    assert [p['matrix'] for p in value['skin']['palette']]==[[[-1,-1,0,2],[0,1,0,0],[0,0,2,9.5]],[[1,1,0,-1.5],[0,-1,0,4],[0,0,2,7.5]]]
    assert value['skin']['skin_to_source_world']==[[0,-4,0,-4],[4,0,0,-6],[0,0,4,-4]]
    permuted=copy.deepcopy(base);permuted['explicit_bone_mapping'].reverse();assert run('permuted',permuted)==value
    alternate=copy.deepcopy(base);alternate['explicit_bone_mapping'][1]['rig_node']=4
    different=run('same-name-distinct-id',alternate);assert different['skin']['positions'][2]==[9,-10.5,-5.5]
    bad=copy.deepcopy(base);bad['expected_skin_sha256'][0]^=1;run('skin-sha',bad,'skin source SHA256 differs')
    bad=copy.deepcopy(base);bad['expected_rig_sha256'][0]^=1;run('rig-sha',bad,'rig source SHA256 differs')
    bad=copy.deepcopy(base);bad['explicit_bone_mapping'][1]['bone_ordinal']=0;run('duplicate-ordinal',bad,'duplicate skin bone ordinal')
    bad=copy.deepcopy(base);bad['explicit_bone_mapping'][1]['rig_node']=2;run('duplicate-target',bad,'duplicate rig node target')
    for label,field in [('skin-name','expected_skin_bone_name_bytes'),('rig-name','expected_rig_node_name_bytes')]:
        bad=copy.deepcopy(base);bad['explicit_bone_mapping'][1][field].append(0);run(label,bad,'raw name differs')
    bad=copy.deepcopy(base);bad['explicit_bone_mapping'].pop();run('incomplete',bad,'must cover every')
    bad=copy.deepcopy(base);bad['rig_root']=3;run('root',bad,'does not reach chosen rig root')
    bad=copy.deepcopy(base);bad['explicit_bone_mapping'][1]['rig_node']=0;run('orphan',bad,'no reachable ancestry')
    bad=copy.deepcopy(base);bad['explicit_root_space_mapping'][0]=[0,0,0,0];run('singular',bad,'root-space mapping is singular')
    run('geometry',dict(base,geometry=1),'selected geometry has no decoded skin owner')
    bad=copy.deepcopy(base);bad['explicit_bone_mapping'][1]['bone_ordinal']=2;run('ordinal-range',bad,'bone ordinal out of range')
    bad=copy.deepcopy(base);bad['explicit_bone_mapping'][1]['rig_node']=999;run('node-range',bad,'rig node out of range')
    changed=rig_blocks.copy();data=bytearray(changed[1][1]);data[80:84]=writer.words(1);changed[1]=('NiNode',bytes(data))
    cyclic=rig_dir/'cycle.nif';cyclic.write_bytes(container(changed,[0],[b'Rig\0',b'Twin']));frozen[str(cyclic)]=sha(cyclic)
    run('cycle',dict(base,expected_rig_sha256=list(bytes.fromhex(sha(cyclic)))),'cycle in NIF scene children',rig_input=cyclic)
    run('schema',dict(base,schema_version=2),'schema');run('extra',dict(base,extra=1),'schema')
    run('policy',dict(base,weights=dict(kind='auto_normalize')),'schema')
    for label,folder in [('skin',skin_dir),('rig',rig_dir),('request',requests)]:run('protected-'+label,base.copy(),'protected',destination=folder/'blocked-output.json')
    for path,digest in frozen.items():assert sha(Path(path))==digest
    result=dict(contract='engineering-exact-external-rig-skin-v1',analytic_cases=2,permutation_cases=1,
        intended_refusals=sum(i['expected_error'] is not None for i in invocations),invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    arguments=parser.parse_args();result=check(arguments.binary,arguments.output)
    print(json.dumps({key:value for key,value in result.items() if key not in ('invocations','frozen_hashes')}))
