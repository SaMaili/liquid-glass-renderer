"""Assemble explicit AIR inline-SDF wrappers around reviewed material ports."""
from pathlib import Path
ROOT=Path(__file__).resolve().parent
HELPER=(ROOT/'include/inline-sdf.glsl').read_text()
BACKGROUND=('minimal','edge','refraction','refraction-edge','composite','composite-edge','composite-refraction','all')

def build():
    for name in BACKGROUND:
        src=(ROOT/f'background-{name}.frag').read_text()
        src=src.replace('layout(location=0) in vec2 sourceUV;','layout(location=1) in vec2 sourceUV;')
        src=src.replace('layout(location=1) in vec2 sdfUV;','layout(location=0) in vec2 sdfUV;')
        src=src.replace('layout(binding=4) uniform sampler2D sdfTexture;','')
        src=src.replace('uvec4 words[20]','uvec4 words[23]')
        src=src.replace('return u.words[byteOffset/16][(byteOffset%16)/4];','byteOffset+=48; return u.words[byteOffset/16][(byteOffset%16)/4];')
        helper='vec4 geometry(int i) { return uintBitsToFloat(u.words[i]); }\n'+HELPER
        src=src.replace('void main() {',helper+'\nvoid main() {')
        # ring helper precedes main, so declare the functions before it is used.
        src=src.replace('float sat(float x)', 'vec4 geometry(int i);\nvec4 computeSdf(vec2 p,int mode);\nfloat sat(float x)')
        src=src.replace('texture(sdfTexture,sdfUV+U2(64))','computeSdf(sdfUV+U2(64),abs(int(geometry(0).z))|4)')
        src=src.replace('texture(sdfTexture,sdfUV+U2(240))','computeSdf(sdfUV+U2(240),abs(int(geometry(0).z))|4)')
        src=src.replace('texture(sdfTexture,sdfUV)','(int(geometry(0).z)>=0 ? computeSdf(sdfUV,int(geometry(0).z)) : vec4(0))')
        assert 'sdfTexture' not in src
        src=src.replace('// Semantic port', '// Inline-SDF wrapper from modules 0065–0072; material below is a semantic port',1)
        (ROOT/f'background-sdf-{name}.frag').write_text(src)
    src=(ROOT/'foreground.frag').read_text()
    src=src.replace('layout(location=0) in vec2 sourceUV;','layout(location=1) in vec2 sourceUV;').replace('layout(location=1) in vec2 sdfUV;','layout(location=0) in vec2 sdfUV;')
    src=src.replace('layout(binding=4) uniform sampler2D sdfTexture;','')
    src=src.replace('    vec4 displacement_mat;','    vec4 geometry_data[3];\n    vec4 displacement_mat;',1)
    src=src.replace('void main() {','vec4 geometry(int i) { return u.geometry_data[i]; }\n'+HELPER+'\nvoid main() {')
    src=src.replace('texture(sdfTexture,sdfUV)','computeSdf(sdfUV,int(abs(geometry(0).z)))')
    src=src.replace('// Semantic port', '// Inline-SDF wrapper from module 0074; semantic port',1)
    (ROOT/'foreground-sdf.frag').write_text(src)
    probe='''#version 450
layout(location=0) in vec2 sdfUV;
layout(location=0) out vec4 outputColor;
layout(std140,binding=1) uniform Geometry { vec4 data[3]; } u;
vec4 geometry(int i) { return u.data[i]; }
'''+HELPER+'\nvoid main(){outputColor=computeSdf(sdfUV,int(abs(geometry(0).z)));}\n'
    (ROOT/'sdf-probe.frag').write_text(probe)

if __name__=='__main__':build()
