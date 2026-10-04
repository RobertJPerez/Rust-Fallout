"""Second authored sources and literal sampled rigid attachment transforms."""
import argparse,copy,hashlib,json,struct,subprocess
from pathlib import Path
import check_attachment as writer

words=writer.words;floats=writer.floats;NAME=writer.NAME;NULL=writer.NULL
HALF=[-1,0,0,0,-1,0,0,0,1];ROT=[0,-1,0,1,0,0,0,0,1]
sha=writer.digest
def node(name,children,translation,rotation,scale,controller=NULL):
    raw=bytearray(writer.node(name,children,translation,rotation,scale));raw[8:12]=words(controller);return bytes(raw)
def container(blocks,strings):
    types=list(dict.fromkeys(t for t,_ in blocks))
    raw=b'Gamebryo File Format, Version 20.2.0.7\n'+words(0x14020007)+bytes([1])+words(11,len(blocks),34)+bytes(3)+struct.pack('<H',len(types))
    for t in types:raw+=words(len(t))+t.encode()
    raw+=struct.pack('<'+'H'*len(blocks),*(types.index(t) for t,_ in blocks))+words(*(len(p) for _,p in blocks))+words(len(strings),max(map(len,strings)))
    for name in strings:raw+=words(len(name))+name
    raw+=words(0);spans={};offset=len(raw)
    for i,(_,payload) in enumerate(blocks):
        spans[i]=dict(block=i,offset=offset,bytes=len(payload),sha256=hashlib.sha256(payload).hexdigest());raw+=payload;offset+=len(payload)
    return raw+words(1,0),spans
def sources(scale=None,ancestor=False,child=False,attachment_child=False,chain=False):
    controller=words(2 if chain else NULL)+struct.pack('<H',0xFFFF)+floats(17,-9,100,101)+words(1,3)
    interpolator=floats(1000,2000,3000,2,-3,4,-5,12)+words(4)
    keys=words(0,2,1)+floats(0,-2,3,1,2,6,-1,5)+words(2,1)+floats(0,1 if scale is None else scale,2,3 if scale is None else scale)
    blocks=[('NiNode',node(0,[1],[2,-3,5],HALF,3,2 if ancestor else NULL)),
            ('NiNode',node(1,[5] if child else [],[1,4,-2],ROT,2,2)),('NiTransformController',controller),('NiTransformInterpolator',interpolator),('NiTransformData',keys)]
    if child:blocks.append(('NiNode',node(1,[],[0,0,0],ROT,1,2)))
    attachment=[('NiNode',node(0,[1] if attachment_child else [],[-5,8,10],HALF,-2))]
    if attachment_child:attachment.extend([('NiNode',node(0,[],[0,0,0],ROT,1,2)),('NiFloatInterpolator',bytes([0]))])
    return container(blocks,[b'root',NAME]),container(attachment,[b'asset'])

def check(binary,output,previous_binary=None):
    binary=binary.resolve();output.mkdir(parents=True,exist_ok=False);inputs=output/'inputs';inputs.mkdir()
    skel_inputs=inputs/'skeleton';skel_inputs.mkdir();asset_inputs=inputs/'attachment';asset_inputs.mkdir();requests=inputs/'requests';requests.mkdir()
    skeleton=skel_inputs/'source.nif';attachment=asset_inputs/'source.nif';(s,s_spans),(a,a_spans)=sources();skeleton.write_bytes(s);attachment.write_bytes(a)
    frozen={str(p):sha(p) for p in [binary,skeleton,attachment]};invocations=[]
    base=dict(schema_version=1,expected_skeleton_sha256=list(bytes.fromhex(sha(skeleton))),expected_attachment_sha256=list(bytes.fromhex(sha(attachment))),node=1,node_name_bytes=list(NAME),attachment_root=0,
              attachment_parent_to_node=[[1,2,0,3],[0,-1,0,-4],[0,0,.5,7]],source_policy='stored_ni_av_locals',object=1,controller=2,source_time=1)
    def run(name,request,error=None,skeleton_path=skeleton,attachment_path=attachment,destination=None):
        path=requests/(name+'.json');path.write_text(json.dumps(request)+'\n');frozen[str(path)]=sha(path);destination=destination or output/(name+'.json')
        command=[str(binary),'nif-rigid-attachment',str(skeleton_path),'--sampled',str(attachment_path),'--request',str(path),'--output',str(destination)]
        p=subprocess.run(command,capture_output=True,text=True)
        if error in ('schema','protected'):
            assert p.returncode!=0 and not destination.exists(),p.stdout+p.stderr;value=None
        else:
            report=json.loads(destination.read_text());assert p.returncode==(0 if error is None else 1),p.stderr
            if error:assert report['evaluation'] is None and error in report['error'],report;value=None
            else:
                value=report['evaluation'];assert value['contract']=='engineering-one-source-sampled-rigid-attachment-v1' and not value['retail_behavior_verified']
                assert value['sample']['requested_time_f64_bits']==struct.unpack('<Q',struct.pack('<d',request['source_time']))[0]
                assert value['stored_binding']['node_name_bytes']==list(NAME)
                assert value['sample']['source_sha256']==value['stored_binding']['skeleton_sha256']==sha(skeleton_path)
                assert value['stored_binding']['attachment_sha256']==sha(attachment_path)
        invocations.append(dict(command=command,exit_code=p.returncode,expected_error=error,receipt_sha256=sha(destination) if destination.exists() else None));return value
    literal=[('first',0,[[0,-3,0,-4],[-3,-6,0,-21],[0,0,1.5,29]],[[0,-6,0,-28],[-6,-12,0,-54],[0,0,-3,44]]),
             ('middle',1,[[0,-6,0,-28],[-6,-12,0,-24],[0,0,3,56]],[[0,-12,0,-76],[-12,-24,0,-90],[0,0,-6,86]]),
             ('last',2,[[0,-9,0,-52],[-9,-18,0,-27],[0,0,4.5,83]],[[0,-18,0,-124],[-18,-36,0,-126],[0,0,-9,128]])]
    for name,time,mapping,root in literal:
        value=run(name,dict(base,source_time=time));assert value['attachment_source_to_skeleton_source']==mapping and value['root_to_skeleton_source']==root
        for field,block in [('object',1),('controller',2),('interpolator',3),('data',4)]:assert value['sample'][field]==s_spans[block]
        assert value['stored_binding']['attachment_root']['source']==a_spans[0]
    middle=value=run('middle-repeat',base)
    assert value['stored_binding']['attachment_source_to_skeleton_source']==[[0,-6,0,-25],[-6,-12,0,-33],[0,0,3,41]]
    assert value['sample']['unapplied_interpolator_fields']['rotation_wxyz_bits']==[struct.unpack('<I',struct.pack('<f',v))[0] for v in [2,-3,4,-5]]
    negative_zero=run('negative-zero',dict(base,source_time=-0.0));assert negative_zero['attachment_source_to_skeleton_source']==literal[0][2]
    for label,scale,mapping,root in [('reflection',-2,[[0,6,0,20],[6,12,0,12],[0,0,-3,-28]],[[0,12,0,68],[12,24,0,78],[0,0,6,-58]]),
                                      ('zero',0,[[0,0,0,-4],[0,0,0,-6],[0,0,0,14]],[[0,0,0,-4],[0,0,0,-6],[0,0,0,14]])]:
        source=skel_inputs/(label+'.nif');source.write_bytes(sources(scale=scale)[0][0]);frozen[str(source)]=sha(source)
        value=run(label,dict(base,expected_skeleton_sha256=list(bytes.fromhex(sha(source)))),skeleton_path=source)
        assert value['attachment_source_to_skeleton_source']==mapping and value['root_to_skeleton_source']==root
    for label,patch,error in [('stale-skeleton',dict(expected_skeleton_sha256=[0]*32),'skeleton source SHA256 differs'),('stale-attachment',dict(expected_attachment_sha256=[0]*32),'attachment source SHA256 differs'),
                              ('object',dict(object=0),'object differs'),('controller',dict(controller=3),'object.controller differs'),('raw-name',dict(node_name_bytes=list(b'weaponsocket')),'raw node name differs'),
                              ('root',dict(attachment_root=1),'not an exact footer root'),('before',dict(source_time=-1),'extrapolate'),('after',dict(source_time=3),'extrapolate')]:run(label,dict(base,**patch),error)
    for label,options,error in [('ancestor',dict(ancestor=True),'ancestor 0 controller is unapplied'),('socket-child',dict(child=True),'descendant 5 controller 2 is unapplied'),
                                 ('attachment-child',dict(attachment_child=True),'descendant 1 controller 2 is unapplied'),('chain',dict(chain=True),'controller chain is unapplied')]:
        (s,_),(a,_)=sources(**options);sp=skel_inputs/(label+'.nif');ap=asset_inputs/(label+'.nif');sp.write_bytes(s);ap.write_bytes(a)
        frozen[str(sp)]=sha(sp);frozen[str(ap)]=sha(ap)
        run(label,dict(base,expected_skeleton_sha256=list(bytes.fromhex(sha(sp))),expected_attachment_sha256=list(bytes.fromhex(sha(ap)))),error,skeleton_path=sp,attachment_path=ap)
    run('schema',dict(base,schema_version=2),'schema');run('extra',dict(base,extra=True),'schema')
    missing=base.copy();del missing['source_time'];run('missing-time',missing,'schema')
    for label,path in [('skeleton',skel_inputs),('attachment',asset_inputs),('request',requests)]:run('protected-'+label,base.copy(),'protected',destination=path/'blocked.json')
    prior=[]
    if previous_binary is not None:
        previous_binary=previous_binary.resolve();frozen[str(previous_binary)]=sha(previous_binary)
        stored={k:v for k,v in base.items() if k not in ('object','controller','source_time')};path=requests/'stored.json';path.write_text(json.dumps(stored)+'\n');frozen[str(path)]=sha(path)
        receipts=[]
        for label,exe in [('old',previous_binary),('new',binary)]:
            destination=output/(label+'-stored.json');command=[str(exe),'nif-rigid-attachment',str(skeleton),str(attachment),'--request',str(path),'--output',str(destination)]
            p=subprocess.run(command,capture_output=True,text=True);assert p.returncode==0,p.stderr;receipts.append(destination.read_bytes());invocations.append(dict(command=command,exit_code=0,receipt_sha256=sha(destination)))
        assert receipts[0]==receipts[1];prior.append('stored-only-attachment')
        stored_value=json.loads(receipts[1])['evaluation'];assert stored_value==middle['stored_binding']
    for path,digest in frozen.items():assert sha(Path(path))==digest
    result=dict(contract='engineering-authored-sampled-attachment-proof-v1',literal_mappings=5,signed_zero_time_preserved=True,intended_refusals=sum(p.get('expected_error') is not None for p in invocations),
                prior_receipts_byte_equal=prior,invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--previous-binary',type=Path)
    args=p.parse_args();result=check(args.binary,args.output,args.previous_binary);print(json.dumps({k:v for k,v in result.items() if k not in ('invocations','frozen_hashes')}))
