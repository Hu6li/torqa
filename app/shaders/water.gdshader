shader_type spatial;
// Rivers and streams: dark, glossy water with slow ripples.

uniform vec3 water_color : source_color = vec3(0.09, 0.2, 0.27);

varying vec3 world_position;

void vertex() {
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

void fragment() {
	vec2 p = world_position.xz * 0.35;
	float t = TIME * 0.6;
	vec3 ripple = vec3(sin(p.x + t) * 0.06 + sin(p.y * 1.7 - t * 1.3) * 0.04, 1.0,
		cos(p.y + t * 0.8) * 0.06 + cos(p.x * 1.3 + t) * 0.04);
	NORMAL = normalize((VIEW_MATRIX * vec4(normalize(ripple), 0.0)).xyz);
	ALBEDO = water_color;
	ROUGHNESS = 0.06;
	SPECULAR = 0.7;
}
