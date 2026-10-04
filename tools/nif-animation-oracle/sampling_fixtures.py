"""Independent authored component source inputs, without either sampler."""
import struct
from fixtures import container,w

def bits(value):return struct.unpack("<I",struct.pack("<f",value))[0]
def key(time,*values):return (bits(time),tuple(bits(value) for value in values))
CASES={
    "ordinary":(1,[key(-2.,-0.,1.,-2.),key(0.,8.,-4.,2.),key(3.,-4.,2.,0.)],1,[key(-2.,-0.),key(0.,8.),key(3.,-4.)]),
    "constant":(5,[key(-2.,-0.,1.,-2.),key(0.,8.,-4.,2.),key(3.,-4.,2.,0.)],5,[key(-2.,-0.),key(0.,8.),key(3.,-4.)]),
    "extremes":(1,[(bits(0.),(0x7f7fffff,1,0x80000001)),(bits(1.),(0xff7fffff,2,0x80000002))],1,[(bits(0.),(0x7f7fffff,)),(bits(1.),(0xff7fffff,))]),
    "time_extremes":(1,[(0xff7fffff,(0,1,0x80000001)),(0x7f7fffff,(bits(1.),2,0x80000002))],1,[(0xff7fffff,(0,)),(0x7f7fffff,(bits(1.),))]),
    "tiny_times":(1,[(0,(0,1,0x80000001)),(1,(1,2,0x80000002)),(2,(2,3,0x80000003))],1,[(0,(0,)),(1,(1,)),(2,(2,))]),
    "near_cancellation":(1,[(bits(0.),(0x7f7fffff,0x7f7ffffe,0xff7fffff)),(bits(1.),(0xff7ffffe,0xff7fffff,0x7f7ffffe))],1,[(bits(0.),(0x7f7fffff,)),(bits(1.),(0xff7ffffe,))]),
    "uneven":(1,[key(-10.,-4.,1.,-8.),key(-.25,10.,-0.,2.),key(5.75,-1.,2.,1.),key(19.5,8.,4.,-2.)],1,[key(-10.,-4.),key(-.25,10.),key(5.75,-1.),key(19.5,8.)]),
    "single":(1,[key(2.,-0.,1.,-2.)],5,[key(2.,-0.)]),
    "absent":(1,[],1,[]),
}
REFUSALS={
    "duplicate":(1,[key(0.,0.,1.,2.),key(-0.,3.,4.,5.)],1,[]),
    "unsorted":(1,[key(2.,0.,1.,2.),key(0.,3.,4.,5.)],1,[]),
    "quadratic":(2,[key(0.,0.,1.,2.),key(1.,3.,4.,5.)],1,[]),
    "tbc":(3,[key(0.,0.,1.,2.),key(1.,3.,4.,5.)],1,[]),
}
def group(tag,keys):
    result=w(len(keys))
    if keys:result+=w(tag)
    for time,values in keys:
        result+=w(time,*values)
        if tag==2:result+=w(*([0]*len(values)*2))
        if tag==3:result+=w(0,0,0)
    return result
def source(case):
    translation_tag,translation,scale_tag,scale=case
    payload=w(0)+group(translation_tag,translation)+group(scale_tag,scale)
    return container(34,[("NiTransformData",payload),("NiNode",b"")],[])
