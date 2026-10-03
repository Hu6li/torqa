shader_type spatial;
// Lakes and rivers: deep teal water that reflects the sky more at grazing angles (fresnel),
// with small moving ripples.

uniform vec3 deep_color : source_color = vec3(0.05, 0.22, 0.32);
uniform vec3 shallow_color : source_color = vec3(0.12, 0.38, 0.48);
uniform vec3 sky_tint : source_color = vec3(0.55, 0.72, 0.9);

varying vec3 world_position;

float ripple(vec2 p, float t) {
	return sin(p.x * 0.9 + t) * 0.5 + sin(p.y * 1.3 - t * 1.2) * 0.35
		+ sin((p.x + p.y) * 2.1 + t * 1.7) * 0.15;
}

void vertex() {
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

void fragment() {
	vec2 p = world_position.xz * 0.6;
	float t = TIME * 0.8;
	float e = 0.05;
	float h = ripple(p, t);
	vec3 ripple_normal = normalize(vec3(
		(ripple(p + vec2(e, 0.0), t) - h) / e * -0.04, 1.0, (ripple(p + vec2(0.0, e), t) - h) / e * -0.04));
	NORMAL = normalize((VIEW_MATRIX * vec4(ripple_normal, 0.0)).xyz);

	float facing = clamp(dot(normalize(VIEW), NORMAL), 0.0, 1.0);
	float fresnel = pow(1.0 - facing, 4.0);
	vec3 water = mix(shallow_color, deep_color, facing);
	ALBEDO = mix(water, sky_tint, fresnel * 0.6);
	ROUGHNESS = 0.04;
	METALLIC = 0.0;
	SPECULAR = 0.6;
}
