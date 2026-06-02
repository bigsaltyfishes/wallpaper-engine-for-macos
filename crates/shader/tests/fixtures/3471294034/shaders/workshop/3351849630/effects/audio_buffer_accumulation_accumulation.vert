attribute vec3 a_Position;
attribute vec2 a_TexCoord;

varying vec4 v_TexCoord;
varying vec2 v_AccumulationRate;

void main() {
    gl_Position = vec4(a_Position, 1.0);
    v_TexCoord.xyzw = a_TexCoord.xyxy;
    v_AccumulationRate.x = 0.85;
    v_AccumulationRate.y = 0.90;
}
