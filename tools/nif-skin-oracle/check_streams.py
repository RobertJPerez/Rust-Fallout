"""Independent authored partition rows and topology through the public CLI."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess

NULL = 0xFFFFFFFF

def words(*values): return struct.pack('<'+'I'*len(values), *values)
def shorts(*values): return struct.pack('<'+'H'*len(values), *values)
def floats(*values): return struct.pack('<'+'f'*len(values), *values)
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def av():
    return words(NULL,0,NULL,14)+floats(0,0,0,1,0,0,0,1,0,0,0,1,1)+words(0,NULL)
def node(children=()): return av()+words(len(children),*children,0)
def geometry():
    return words(0)+shorts(3)+bytes([0,0,0])+shorts(0)+bytes([0])+floats(0,0,0,1)+bytes([0])+shorts(0)+words(NULL)+shorts(0)+words(0)+bytes([0])+shorts(0)
def packet(strip=False, mapping=(1,2,0), palette=(1,0,1), presence=True,
           weight_bits=(0xBF000000,0,0x3F400000,0x40000000),
           indices=(2,1,2,0, 0,2,1,2, 1,0,2,1), face=(2,0,1)):
    header=shorts(3,1,len(palette),int(strip),4,*palette)
    if not presence:return header+bytes([0,0,0,0])
    raw=header+bytes([1])+shorts(*mapping)+bytes([1])+words(*(weight_bits*3))
    if strip: raw+=shorts(4)
    return raw+bytes([1])+shorts(*(face+(face[-1],) if strip else face))+bytes([1])+bytes(indices)
def fixture(packets=None, bone0=5):
    packets=packets or [packet(),packet(True),packet(mapping=(1,1,0))]
    blocks=[('NiNode',node((2,5,6))),('NiSkinInstance',words(NULL,4,0,2,bone0,6)),
            ('NiTriShape',av()+words(3,1,0,NULL)+bytes([0])),('NiTriShapeData',geometry()),
            ('NiSkinPartition',words(len(packets))+b''.join(packets)),('NiNode',node()),('NiNode',node())]
    types=list(dict.fromkeys(name for name,_ in blocks))
    raw=b'Gamebryo File Format, Version 20.2.0.7\n'+words(0x14020007)+bytes([1])+words(11,len(blocks),34)+bytes([0,0,0])
    raw+=shorts(len(types))+b''.join(words(len(name))+name.encode() for name in types)
    raw+=shorts(*(types.index(name) for name,_ in blocks))+words(*(len(data) for _,data in blocks))+words(0,0,0)
    offset=len(raw);spans={}
    for block,(_,data) in enumerate(blocks):
        spans[block]=dict(block=block,offset=offset,bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
        raw+=data;offset+=len(data)
    return raw+words(1,0),spans

def check(binary,output,previous_binary=None):
    binary=binary.resolve();output.mkdir(parents=True,exist_ok=False)
    inputs=output/'inputs';inputs.mkdir();requests=output/'requests';requests.mkdir()
    source=inputs/'authored.nif';raw,spans=fixture();source.write_bytes(raw)
    frozen={str(binary):sha(binary),str(source):sha(source)}
    invocations=[]
    base=dict(schema_version=1,expected_source_sha256=list(bytes.fromhex(sha(source))),geometry=2,partition_block=4,partition_ordinal=0)
    def run(name,request,error=None,input_path=source,destination=None,extra=()):
        path=requests/(name+'.json');path.write_text(json.dumps(request)+'\n');frozen[str(path)]=sha(path)
        destination=destination or output/(name+'.json')
        command=[str(binary),'nif-skin',str(input_path),'--partition-streams-request',str(path),*extra,'--output',str(destination)]
        process=subprocess.run(command,capture_output=True,text=True)
        if error in ('schema','protected','conflict'):
            assert process.returncode!=0 and not destination.exists(),process.stdout+process.stderr
            value=None
        else:
            report=json.loads(destination.read_text());assert process.returncode==(0 if error is None else 1),process.stderr
            if error:
                assert report['evaluation'] is None and error in report['error'],report;value=None
            else:
                value=report['evaluation'];assert value['contract']=='source-qualified-authored-partition-streams-v1' and not value['retail_behavior_verified']
        invocations.append(dict(command=command,exit_code=process.returncode,expected_error=error,receipt_sha256=sha(destination) if destination.exists() else None))
        return value
    def literal(value,ordinal,mapping,strip):
        assert value['identity']==dict(source_sha256=sha(source),geometry=spans[2],geometry_data=spans[3],instance=spans[1],partition=spans[4],partition_ordinal=ordinal,source_vertex_count=3)
        assert value['presence']==dict(vertex_map=1,vertex_weights=1,faces=1,bone_indices=1)
        assert [value[k] for k in ('declared_vertices','declared_triangles','declared_bones','declared_strips','weights_per_vertex')]==[3,1,3,int(strip),4]
        assert value['palette']==[dict(local_bone=0,global_bone_ordinal=1,source_bone_node=6),dict(local_bone=1,global_bone_ordinal=0,source_bone_node=5),dict(local_bone=2,global_bone_ordinal=1,source_bone_node=6)]
        assert value['vertices']==[dict(local_vertex=i,source_vertex=v,influence_start=i*4,influence_count=4) for i,v in enumerate(mapping)]
        # Literal independently authored global bone/node outcomes, including duplicates.
        bones=[(2,1,6),(1,0,5),(2,1,6),(0,1,6),(0,1,6),(2,1,6),(1,0,5),(2,1,6),(1,0,5),(0,1,6),(2,1,6),(1,0,5)]
        expected=[]
        for i,(local,global_bone,node_id) in enumerate(bones):
            expected.append(dict(source_weight_ordinal=i,local_vertex=i//4,source_vertex=mapping[i//4],slot=i%4,local_bone=local,
                                 global_bone_ordinal=global_bone,source_bone_node=node_id,weight_bits=[0xBF000000,0,0x3F400000,0x40000000][i%4]))
        assert value['influences']==expected
        if strip:
            assert value['topology']==dict(kind='strips',lengths=[4],strips=[dict(local_vertices=[2,0,1,1],source_vertices=[0,1,2,2])])
        else:
            assert value['topology']==dict(kind='triangles',triangles=[dict(local_vertices=[2,0,1],source_vertices=[mapping[2],mapping[0],mapping[1]])])
        assert (value['usage']['influence_rows'],value['usage']['draw_indices'])==(12,4 if strip else 3)
    literal(run('triangles',base),0,[1,2,0],False)
    literal(run('strips',dict(base,partition_ordinal=1)),1,[1,2,0],True)
    literal(run('duplicate-map',dict(base,partition_ordinal=2)),2,[1,1,0],False)
    wrong=copy.deepcopy(base);wrong['expected_source_sha256'][0]^=1;run('stale',wrong,'SHA256 differs')
    run('geometry',dict(base,geometry=0),'no decoded skin owner')
    run('partition',dict(base,partition_block=5),'partition link differs')
    run('ordinal',dict(base,partition_ordinal=3),'ordinal unavailable')
    variants=[('absent',dict(packets=[packet(presence=False)]),'requires authored'),
              ('local-bone',dict(packets=[packet(indices=(3,)*12)]),'outside palette'),
              ('global-bone',dict(packets=[packet(palette=(2,0,1))]),'outside linked instance'),
              ('vertex-map',dict(packets=[packet(mapping=(3,2,0))]),'outside linked geometry'),
              ('draw-index',dict(packets=[packet(face=(3,0,1))]),'outside local vertices'),
              ('missing-bone',dict(bone0=NULL),'no source bone link'),
              ('bone-kind',dict(bone0=3),'skin bone link has wrong target kind')]
    for name,args,error in variants:
        bad=inputs/(name+'.nif');bad.write_bytes(fixture(**args)[0]);frozen[str(bad)]=sha(bad)
        run(name,dict(base,expected_source_sha256=list(bytes.fromhex(sha(bad)))),error,input_path=bad)
    run('schema',dict(base,schema_version=2),'schema');run('extra',dict(base,extra=True),'schema')
    run('conflict',base.copy(),'conflict',extra=['--include-partitions'])
    for label,folder in [('input',inputs),('request',requests)]:
        run('protected-'+label,base.copy(),'protected',destination=folder/'blocked.json')
    prior=[]
    if previous_binary is not None:
        previous_binary=previous_binary.resolve();frozen[str(previous_binary)]=sha(previous_binary)
        for schema,flags in [(1,[]),(2,['--include-partitions']),(3,['--include-bindings'])]:
            values=[]
            for label,exe in [('old',previous_binary),('new',binary)]:
                destination=output/f'{label}-schema{schema}.json';command=[str(exe),'nif-skin',str(source),*flags,'--output',str(destination)]
                process=subprocess.run(command,capture_output=True,text=True);assert process.returncode==0,process.stderr
                values.append(destination.read_bytes());invocations.append(dict(command=command,exit_code=0,receipt_sha256=sha(destination)))
            assert values[0]==values[1];prior.append(schema)
    for path,digest in frozen.items():assert sha(Path(path))==digest
    result=dict(contract='engineering-authored-partition-streams-proof-v1',literal_partitions=3,literal_rows=36,prior_schemas_byte_equal=prior,
                intended_refusals=sum(p.get('expected_error') is not None for p in invocations),invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--previous-binary',type=Path)
    args=parser.parse_args();result=check(args.binary,args.output,args.previous_binary)
    print(json.dumps({k:v for k,v in result.items() if k not in ('invocations','frozen_hashes')}))
