import type { RiskTone } from './presentation'

export const LIQUID_TRANSITION_MS = 800
export type LiquidWeights = readonly [number, number, number, number]
export interface LiquidTransition { from: LiquidWeights; to: LiquidWeights; startedAt: number }

export function liquidWeights(tone: RiskTone): LiquidWeights {
  return [Number(tone === 'unknown'), Number(tone === 'aligned'), Number(tone === 'review'), Number(tone === 'deviation')]
}

export function settledLiquidTransition(tone: RiskTone, now: number): LiquidTransition {
  const weights = liquidWeights(tone)
  return { from: weights, to: weights, startedAt: now }
}

export function sampleLiquidTransition(transition: LiquidTransition, now: number): LiquidWeights {
  const progress = Math.max(0, Math.min(1, (now - transition.startedAt) / LIQUID_TRANSITION_MS))
  const eased = progress * progress * (3 - 2 * progress)
  return transition.from.map((value, index) => value + (transition.to[index] - value) * eased) as unknown as LiquidWeights
}

/** Retarget from the pixels currently on screen, including an interrupted transition. */
export function retargetLiquidTransition(transition: LiquidTransition, tone: RiskTone, now: number): LiquidTransition {
  return { from: sampleLiquidTransition(transition, now), to: liquidWeights(tone), startedAt: now }
}

/** The real orb is 70 logical pixels; larger design previews share the bounded backing buffer. */
export function liquidPixelSize(dpr: number) {
  return Math.round(70 * Math.max(1, Math.min(2, Number.isFinite(dpr) ? dpr : 1)))
}

export const LIQUID_VERTEX_SHADER = `
attribute vec2 a_position;
varying mediump vec2 v_position;
void main() {
  v_position = a_position;
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`

// Every surface, ribbon, vein and highlight is procedural. No sampled image or video.
export const LIQUID_FRAGMENT_SHADER = `
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying mediump vec2 v_position;
uniform vec4 u_phase_a;
uniform vec4 u_phase_b;
uniform vec4 u_phase_c;
uniform vec4 u_tone;
uniform float u_resolution;

float hash(vec3 p) {
  p = fract(p * 0.1031);
  p += dot(p, p.yzx + 19.19);
  return fract((p.x + p.y) * p.z);
}
float noise(vec3 p) {
  vec3 i = floor(p);
  vec3 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(mix(hash(i), hash(i + vec3(1,0,0)), f.x),
                 mix(hash(i + vec3(0,1,0)), hash(i + vec3(1,1,0)), f.x), f.y),
             mix(mix(hash(i + vec3(0,0,1)), hash(i + vec3(1,0,1)), f.x),
                 mix(hash(i + vec3(0,1,1)), hash(i + vec3(1,1,1)), f.x), f.y), f.z);
}
float fbm(vec3 p) {
  float value = noise(p) * 0.57;
  p = p * 2.03 + vec3(3.1, 1.7, 2.4);
  value += noise(p) * 0.28;
  p = p * 2.01 + vec3(1.3, 4.1, 1.2);
  return value + noise(p) * 0.15;
}
float ribbon(float field, float width) {
  return 1.0 - smoothstep(width * 0.28, width, abs(field));
}
void main() {
  vec2 p = v_position / 0.988;
  float radius = length(p);
  float aa = 2.0 / u_resolution;
  float alpha = 1.0 - smoothstep(1.0 - aa, 1.0, radius);
  if (radius >= 1.0) { gl_FragColor = vec4(0.0); return; }

  float z = sqrt(max(0.0, 1.0 - dot(p, p)));
  vec3 normal = vec3(p, z);
  vec3 q = vec3(p * 1.6, z * 1.25);
  // A moving domain bends the bands themselves; nothing is a rigid rotating path.
  vec3 drift = vec3(sin(u_phase_a.x) * 0.8, cos(u_phase_a.y) * 0.7, sin(u_phase_a.z) * 0.9);
  float warp = fbm(q * 1.35 + drift) - 0.5;
  q.xy += vec2(sin(q.y * 2.0 + u_phase_a.w), cos(q.x * 2.3 - u_phase_b.x)) * (0.15 + 0.09 * u_tone.w);
  q.xy += vec2(warp, -warp) * 0.4;

  float front = q.x * 0.58 + q.y * 0.28
    + 0.31 * sin(q.y * 2.55 + u_phase_b.y)
    + 0.13 * sin(q.x * 3.8 - q.y * 1.2 - u_phase_b.z) + warp * 0.32;
  float back = q.x * 0.46 - q.y * 0.42
    + 0.36 * sin(q.y * 2.15 - u_phase_b.w + 1.7) - warp * 0.27;
  float band = ribbon(front, 0.34);
  float rearBand = ribbon(back, 0.31);
  float lip = exp(-abs(front - 0.12) * 35.0);
  float rearLip = exp(-abs(back + 0.1) * 31.0);
  float fold = smoothstep(-0.3, 0.16, front);

  vec3 color = mix(vec3(0.013, 0.022, 0.075), vec3(0.075, 0.095, 0.25), z * 0.72 + p.y * 0.1);
  vec3 blue = mix(vec3(0.12, 0.16, 0.52), vec3(0.18, 0.69, 0.94), fold);
  vec3 violet = mix(vec3(0.14, 0.10, 0.37), vec3(0.57, 0.55, 0.98), rearLip);
  color += rearBand * violet * (0.45 + 0.22 * z);
  color = mix(color, blue * (0.5 + 0.48 * z), band * 0.78);
  color += lip * vec3(0.56, 0.91, 1.0) * (0.58 + 0.35 * z);
  color += rearLip * vec3(0.39, 0.4, 0.95) * 0.38;
  float silk = sin(front * 95.0 + warp * 5.0) * 0.5 + 0.5;
  color += band * silk * vec3(0.06, 0.11, 0.15) * 0.17;

  // Warm the front liquid surface itself, so review remains visible wherever it flows.
  // The rear blue/violet ribbon and dark glass retain their own colors.
  float localWarm = 0.45 + 0.55 * smoothstep(-0.68, 0.18, p.y);
  float warmMask = min(1.0, band * 0.82 + lip * 0.7) * localWarm;
  vec3 amber = vec3(0.95, 0.39, 0.055) * (0.42 + 0.5 * z)
    + vec3(1.0, 0.8, 0.34) * lip * 0.8;
  color = mix(color, amber, u_tone.z * warmMask * 0.96);

  float veins = abs(fbm(q * 2.8 + vec3(warp, sin(u_phase_c.x) * 1.3, cos(u_phase_c.y) * 0.9)) - 0.5);
  float branching = (1.0 - smoothstep(0.018, 0.075, veins)) * smoothstep(0.05, 0.5, abs(front));
  vec3 coral = color * vec3(1.32, 0.61, 0.69) + vec3(0.84, 0.24, 0.18) * branching * 0.55;
  color = mix(color, coral, u_tone.w);

  float cloud = fbm(q * 2.2 + vec3(warp * 2.0, sin(u_phase_c.z), cos(u_phase_c.w)));
  vec3 fog = mix(vec3(0.09, 0.12, 0.18), vec3(0.63, 0.69, 0.76), smoothstep(0.2, 0.83, cloud));
  fog *= 0.5 + z * 0.6;
  color = mix(color, fog, u_tone.x);

  // Glass lighting stays continuous while the liquid and tone weights change underneath it.
  vec3 light = normalize(vec3(-0.48, 0.73, 1.25));
  float highlight = pow(max(dot(reflect(-light, normal), vec3(0,0,1)), 0.0), 26.0);
  float fresnel = pow(1.0 - z, 3.0);
  vec3 rimColor = mix(vec3(0.35, 0.43, 0.91), vec3(0.55, 0.62, 0.72), u_tone.x);
  color += rimColor * fresnel * (0.34 + max(p.y, 0.0) * 0.75);
  color += vec3(0.82, 0.9, 1.0) * highlight * 0.8;
  color += vec3(0.22, 0.27, 0.43) * pow(max(dot(normal, light), 0.0), 12.0) * 0.35;
  color = min(color, vec3(1.0));
  gl_FragColor = vec4(color * alpha, alpha);
}
`

export interface LiquidRenderer {
  resize(dpr: number): void
  draw(time: number, weights: LiquidWeights): void
  dispose(): void
}

export function createLiquidRenderer(canvas: HTMLCanvasElement): LiquidRenderer {
  const gl = canvas.getContext('webgl', { alpha: true, antialias: false, depth: false, stencil: false,
    premultipliedAlpha: true, preserveDrawingBuffer: false, powerPreference: 'low-power' })
  if (!gl) throw new Error('WebGL unavailable')
  // The procedural noise needs fractional precision; use the explicit static fallback otherwise.
  if (!gl.getShaderPrecisionFormat(gl.FRAGMENT_SHADER, gl.HIGH_FLOAT)?.precision) throw new Error('Orb shader precision unavailable')
  const shaders: WebGLShader[] = []
  let program: WebGLProgram | null = null
  let buffer: WebGLBuffer | null = null
  const dispose = () => {
    if (buffer) gl.deleteBuffer(buffer)
    if (program) gl.deleteProgram(program)
    for (const shader of shaders) gl.deleteShader(shader)
    buffer = null; program = null; shaders.length = 0
  }
  try {
    for (const [type, source] of [[gl.VERTEX_SHADER, LIQUID_VERTEX_SHADER], [gl.FRAGMENT_SHADER, LIQUID_FRAGMENT_SHADER]] as const) {
      const shader = gl.createShader(type)
      if (!shader) throw new Error('Cannot allocate orb shader')
      shaders.push(shader)
      gl.shaderSource(shader, source); gl.compileShader(shader)
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) ?? 'Orb shader compilation failed')
    }
    program = gl.createProgram()
    if (!program) throw new Error('Cannot allocate orb program')
    for (const shader of shaders) gl.attachShader(program, shader)
    gl.linkProgram(program)
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) ?? 'Orb shader link failed')
    buffer = gl.createBuffer()
    if (!buffer) throw new Error('Cannot allocate orb buffer')
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW)
    const position = gl.getAttribLocation(program, 'a_position')
    gl.enableVertexAttribArray(position)
    gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0)
    const phaseA = gl.getUniformLocation(program, 'u_phase_a')
    const phaseB = gl.getUniformLocation(program, 'u_phase_b')
    const phaseC = gl.getUniformLocation(program, 'u_phase_c')
    const toneUniform = gl.getUniformLocation(program, 'u_tone')
    const resolutionUniform = gl.getUniformLocation(program, 'u_resolution')
    gl.useProgram(program)
    gl.disable(gl.DEPTH_TEST)
    gl.disable(gl.BLEND)
    return {
      resize(dpr) {
        const size = liquidPixelSize(dpr)
        if (canvas.width !== size || canvas.height !== size) { canvas.width = size; canvas.height = size }
        gl.viewport(0, 0, size, size)
        gl.uniform1f(resolutionUniform, size)
      },
      draw(time, weights) {
        if (!program || gl.isContextLost()) return
        // Keep shader inputs small even on WebGL1 devices with mediump fragment floats.
        const phase = (rate: number) => time * rate % (Math.PI * 2)
        gl.uniform4f(phaseA, phase(.31), phase(.27), phase(.23), phase(.72))
        gl.uniform4f(phaseB, phase(.61), phase(.69), phase(.83), phase(.57))
        gl.uniform4f(phaseC, phase(.13), phase(.09), phase(.32), phase(.29))
        gl.uniform4f(toneUniform, ...weights)
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4)
      },
      dispose,
    }
  } catch (error) { dispose(); throw error }
}
