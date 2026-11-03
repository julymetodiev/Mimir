// Layout in compute passes and drawing. See docs/ARCHITECTURE.md section 7.
struct Params {
    nodes: u32, edges: u32, grid: u32, focused: u32,
    aspect: f32, zoom: f32, centre_x: f32, centre_y: f32,
}
struct Node { pos: vec2<f32>, vel: vec2<f32>, home: vec2<f32> }
struct Edge { a: u32, b: u32, kind: u32 }

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> nodes: array<Node>;
@group(0) @binding(2) var<storage, read> edges: array<Edge>;
@group(0) @binding(3) var<storage, read_write> force: array<atomic<i32>>;
@group(0) @binding(4) var<storage, read_write> density: array<atomic<i32>>;
@group(0) @binding(5) var<storage, read_write> heat: array<f32>;

// Forces add up in integers: floats have no atomic add.
const SCALE: f32 = 65536.0;
const REACH: i32 = 8;

fn cell(p: vec2<f32>) -> vec2<i32> {
    let c = (p * 0.5 + 0.5) * f32(params.grid);
    return clamp(vec2<i32>(c), vec2<i32>(REACH), vec2<i32>(i32(params.grid) - REACH - 1));
}
fn index(c: vec2<i32>) -> u32 { return u32(c.y) * params.grid + u32(c.x); }

@compute @workgroup_size(256) fn clear_grid(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x < params.grid * params.grid { atomicStore(&density[id.x], 0); }
}
@compute @workgroup_size(256) fn deposit(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x < params.nodes { atomicAdd(&density[index(cell(nodes[id.x].pos))], 1); }
}
// A spring with a rest length, and no pull across the graph that would close it up.
@compute @workgroup_size(256) fn springs(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params.edges { return; }
    let e = edges[id.x];
    let d = nodes[e.b].pos - nodes[e.a].pos;
    let len = max(length(d), 0.0001);
    let weight = select(0.15, 1.0, e.kind == 0u);
    let f = (d / len) * clamp(len - 0.006, -0.02, 0.02) * weight;
    let fx = i32(f.x * SCALE);
    let fy = i32(f.y * SCALE);
    atomicAdd(&force[e.a * 2u], fx);
    atomicAdd(&force[e.a * 2u + 1u], fy);
    atomicAdd(&force[e.b * 2u], -fx);
    atomicAdd(&force[e.b * 2u + 1u], -fy);
}
@compute @workgroup_size(256) fn integrate(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.nodes { return; }
    var n = nodes[i];
    let c = cell(n.pos);
    var push = vec2<f32>(0.0);
    for (var reach = 1; reach <= REACH; reach *= 2) {
        let l = f32(atomicLoad(&density[index(c + vec2<i32>(-reach, 0))]));
        let r = f32(atomicLoad(&density[index(c + vec2<i32>(reach, 0))]));
        let d = f32(atomicLoad(&density[index(c + vec2<i32>(0, -reach))]));
        let u = f32(atomicLoad(&density[index(c + vec2<i32>(0, reach))]));
        push += vec2<f32>(l - r, d - u) / f32(reach);
    }
    let pull = vec2<f32>(f32(atomicExchange(&force[i * 2u], 0)), f32(atomicExchange(&force[i * 2u + 1u], 0))) / SCALE;
    n.vel = (n.vel + push * 0.000012 + pull * 0.01 + (n.home - n.pos) * 0.012) * 0.8;
    let speed = length(n.vel);
    if speed > 0.004 { n.vel *= 0.004 / speed; }
    n.pos = clamp(n.pos + n.vel, vec2<f32>(-0.98), vec2<f32>(0.98));
    nodes[i] = n;
}
// What was touched glows, and fades. Negative is an edit.
@compute @workgroup_size(256) fn fade(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x < params.nodes { heat[id.x] *= 0.975; }
}

struct View { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32>, @location(1) uv: vec2<f32> }

@group(0) @binding(0) var<uniform> view: Params;
@group(0) @binding(1) var<storage, read> shown: array<Node>;
@group(0) @binding(2) var<storage, read> shown_edges: array<Edge>;
// A symbol's kind, and above it 1 if related to the chosen symbol, 2 if it is the chosen one.
@group(0) @binding(3) var<storage, read> flags: array<u32>;
@group(0) @binding(4) var<storage, read> glow: array<f32>;

fn place(p: vec2<f32>) -> vec2<f32> {
    let q = (p - vec2<f32>(view.centre_x, view.centre_y)) * view.zoom;
    return vec2<f32>(q.x / max(view.aspect, 1.0), q.y * min(view.aspect, 1.0));
}
fn glow_color(h: f32) -> vec3<f32> {
    return select(vec3<f32>(0.45, 0.85, 1.0), vec3<f32>(1.0, 0.35, 0.3), h < 0.0);
}
@vertex fn edge_vs(@builtin(vertex_index) v: u32) -> View {
    let e = shown_edges[v / 2u];
    var out: View;
    out.pos = vec4<f32>(place(shown[select(e.a, e.b, (v & 1u) == 1u)].pos), 0.0, 1.0);
    var c = vec3<f32>(0.18, 0.83, 0.75);
    if e.kind == 1u { c = vec3<f32>(0.98, 0.75, 0.14); }
    if e.kind >= 2u { c = vec3<f32>(0.39, 0.45, 0.55); }
    var alpha = clamp(0.07 * sqrt(view.zoom), 0.07, 0.35);
    if view.focused != 0u {
        let fa = flags[e.a] >> 8u;
        let fb = flags[e.b] >> 8u;
        alpha = select(0.012, 0.95, (fa == 2u && fb >= 1u) || (fb == 2u && fa >= 1u));
    }
    let hot = max(abs(glow[e.a]), abs(glow[e.b]));
    if hot > 0.02 && min(abs(glow[e.a]), abs(glow[e.b])) > 0.02 {
        c = mix(c, glow_color(glow[e.a] + glow[e.b]), hot);
        alpha = max(alpha, hot * 0.9);
    }
    out.color = vec4<f32>(c, alpha);
    out.uv = vec2<f32>(0.0);
    return out;
}
@fragment fn edge_fs(in: View) -> @location(0) vec4<f32> { return in.color; }

@vertex fn node_vs(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> View {
    var corners = array<vec2<f32>, 6>(vec2(-1.0,-1.0), vec2(1.0,-1.0), vec2(-1.0,1.0), vec2(-1.0,1.0), vec2(1.0,-1.0), vec2(1.0,1.0));
    let corner = corners[v];
    let kind = flags[i] & 255u;
    let focus = flags[i] >> 8u;
    var c = vec3<f32>(1.0, 1.0, 1.0);
    if kind == 0u { c = vec3<f32>(0.18, 0.83, 0.75); }
    if kind == 1u { c = vec3<f32>(0.98, 0.75, 0.14); }
    if kind == 2u { c = vec3<f32>(0.58, 0.64, 0.72); }
    if kind == 3u { c = vec3<f32>(0.96, 0.45, 0.71); }
    var alpha = 0.9;
    var size = 0.0018 * sqrt(view.zoom);
    if view.focused != 0u {
        if focus == 0u { alpha = 0.12; }
        if focus == 1u { size *= 1.6; alpha = 1.0; }
        if focus == 2u { size *= 3.0; alpha = 1.0; c = vec3<f32>(1.0, 1.0, 1.0); }
    }
    let h = glow[i];
    if abs(h) > 0.01 {
        c = mix(c, glow_color(h), min(abs(h) * 1.5, 1.0));
        size *= 1.0 + abs(h) * 4.0;
        alpha = max(alpha, abs(h));
    }
    var out: View;
    let reach = corner * vec2<f32>(size / max(view.aspect, 1.0), size * min(view.aspect, 1.0));
    out.pos = vec4<f32>(place(shown[i].pos) + reach, 0.0, 1.0);
    out.color = vec4<f32>(c, alpha);
    out.uv = corner;
    return out;
}
@fragment fn node_fs(in: View) -> @location(0) vec4<f32> {
    if dot(in.uv, in.uv) > 1.0 { discard; }
    return in.color;
}
