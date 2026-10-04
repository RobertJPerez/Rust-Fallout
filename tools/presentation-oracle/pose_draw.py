"""Authored source-pose triangle packet; no game bytes and no source decoder.

Literal expected matrices/vertices independently specify the draw transform.
The linked-pose producer already tests its own different noncommuting fixtures.
"""
import argparse
from pathlib import Path
import struct

NULL = 2**32 - 1
IDENTITY = [1, 0, 0, 0, 1, 0, 0, 0, 1]

def words(*values):
    return struct.pack('<' + 'I'*len(values), *values)

def floats(*values):
    return struct.pack('<' + 'f'*len(values), *values)

def av(controller, translation, rotation, scale, properties=()):
    return (words(NULL, 0, controller, 14) + floats(*translation, *rotation, scale)
            + words(len(properties), *properties, NULL))

def node(controller, children, translation, rotation, scale):
    return (av(controller, translation, rotation, scale)
            + words(len(children), *children, 0))

def fixture(descendant_controller=NULL):
    # Stored parent +90Z/scale2; selected child half-turnZ. Source key sample
    # time0 yields parent-composed [[0,-2,0,-2],[2,0,0,3],[0,0,-2,5]].
    geometry = (words(0) + struct.pack('<HBBB', 3, 0, 0, 1)
                + floats(0,0,0, 2,0,0, 0,2,0) + struct.pack('<HB', 0, 1)
                + floats(0,0,2, 0,0,2, 0,0,2) + floats(1,1,0,2)
                + b'\x01' + floats(1,0,0,1, 0,1,0,1, 0,0,1,1)
                + struct.pack('<H', 0x4000) + words(NULL)
                + struct.pack('<H', 1) + words(3) + b'\x01'
                + struct.pack('<4H', 0,1,2,0))
    packets = [
        ('NiNode', node(NULL,[1],[2,-3,5],[0,-1,0,1,0,0,0,0,1],2)),
        ('NiNode', node(2,[5],[11,12,13],[-1,0,0,0,-1,0,0,0,1],6)),
        ('NiTransformController', words(NULL)+struct.pack('<H',0xFFFF)
         +floats(9,22,50,51)+words(1,3)),
        ('NiTransformInterpolator', floats(100,200,300,2,-3,4,-5,12)+words(4)),
        ('NiTransformData', words(0,2,1)+floats(-1,4,0,-2,3,0,8,6)
         +words(2,1)+floats(-1,-2,3,2)),
        ('NiTriShape', av(descendant_controller,[1,2,3],[1,0,0,0,0,-1,0,1,0],.5,[7,8])
         +words(6,NULL,0,NULL)+b'\x00'),
        ('NiTriShapeData', geometry),
        ('NiMaterialProperty', words(NULL,0,NULL)+floats(0,0,0,0,0,0,0,1,1)),
        ('NiStencilProperty', words(NULL,0,NULL)+struct.pack('<H',3<<10)+words(0,0)),
    ]
    names = list(dict.fromkeys(name for name, _ in packets))
    header = (b'Gamebryo File Format, Version 20.2.0.7\n'+words(0x14020007)
              +b'\x01'+words(11,len(packets),34)+b'\x00'*3+struct.pack('<H',len(names)))
    for name in names:
        header += words(len(name)) + name.encode('ascii')
    header += struct.pack('<'+'H'*len(packets), *(names.index(name) for name, _ in packets))
    header += words(*(len(packet) for _, packet in packets)) + words(0,0,0)
    return header + b''.join(packet for _, packet in packets) + words(1,0)

# Literal analytic result including the mesh child's +90X/scale1/2 local.
# The geometry owner's old world must never be multiplied a second time.
WORLD_AT_ZERO = [[0,0,1,-6], [1,0,0,5], [0,-1,0,-1]]
DRAW_AT_ZERO = [[-6,-1,-5], [-6,-1,-7], [-6,-3,-5]]
NORMALS_AT_ZERO = [[2,0,0]]*3

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-file', type=Path, required=True)
    parser.add_argument('--descendant-controller', type=int, default=NULL)
    args = parser.parse_args()
    args.output_file.parent.mkdir(parents=True, exist_ok=True)
    with args.output_file.open('xb') as stream:
        stream.write(fixture(args.descendant_controller))

if __name__ == '__main__':
    main()
