shader_type spatial;
// Lakes and rivers (R45, ADR 0011): flat teal water, turning mint where it is seen at a low angle,
// broken into slowly rocking facets for a soft sheen — nothing mirror-like.

uniform vec3 deep_color : source_color = vec3(0.384, 0.514, 0.533);
uniform vec3 shallow_color : source_color = vec3(0.624, 0.812, 0.769);
// Size of the water's facets in metres and how far they tilt.
uniform float facet_size = 7.0;
uniform float facet_tilt = 0.05;

varying vec3 world_position;

float hash(vec2 p) {
	return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

void vertex() {
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

void fragment() {
	// A grid of triangles: each cell split along its diagonal, each half its own facet.
	vec2 cell_position = world_position.xz / facet_size;
	vec2 cell = floor(cell_position);
	vec2 inside = fract(cell_position);
	vec2 facet_id = cell * 2.0 + vec2(step(inside.y, inside.x), 0.0);
	float phase = hash(facet_id) * TAU;
	vec2 tilt = vec2(sin(TIME * 0.6 + phase), cos(TIME * 0.5 + phase * 1.7)) * facet_tilt;
	vec3 world_normal = normalize(vec3(tilt.x, 1.0, tilt.y));
	NORMAL = normalize((VIEW_MATRIX * vec4(world_normal, 0.0)).xyz);
	float facing = clamp(dot(normalize(VIEW), NORMAL), 0.0, 1.0);
	ALBEDO = mix(shallow_color, deep_color, smoothstep(0.1, 0.6, facing));
	ROUGHNESS = 0.35;
	SPECULAR = 0.3;
}
