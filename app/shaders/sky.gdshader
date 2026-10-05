// The sky (R45, ADR 0011): a pastel gradient from the horizon up, a soft sun, and puffy clouds in
// flat cream with shaded undersides drifting overhead, fading into the haze at the horizon.
shader_type sky;

uniform vec3 top_color : source_color = vec3(0.71, 0.859, 0.808);
uniform vec3 horizon_color : source_color = vec3(0.839, 0.925, 0.894);
uniform vec3 ground_color : source_color = vec3(0.698, 0.757, 0.471);
uniform vec3 cloud_color : source_color = vec3(0.984, 0.969, 0.933);
uniform vec3 cloud_shade_color : source_color = vec3(0.78, 0.827, 0.824);
// Share of the sky covered (0 clear, 1 overcast) and how dark the clouds' undersides are.
uniform float cloud_cover : hint_range(0.0, 1.0) = 0.35;
uniform float cloud_darkness : hint_range(0.0, 1.0) = 0.0;
// How far the clouds have drifted, in cloud-layer units (set by the world, slowly).
uniform vec2 cloud_offset = vec2(0.0);
uniform float sun_size : hint_range(0.0, 0.1) = 0.02;

float hash(vec2 p) {
	p = fract(p * vec2(123.34, 456.21));
	p += dot(p, p + 45.32);
	return fract(p.x * p.y);
}

float noise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(
		mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
		mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x),
		u.y
	);
}

float fbm(vec2 p, int octaves) {
	float value = 0.0;
	float amplitude = 0.5;
	for (int i = 0; i < octaves; i++) {
		value += amplitude * noise(p);
		p = p * 2.03 + vec2(17.1, 9.2);
		amplitude *= 0.5;
	}
	return value;
}

void sky() {
	vec3 dir = EYEDIR;
	float up = max(dir.y, 0.0);
	vec3 sky = mix(horizon_color, top_color, pow(up, 0.6));
	if (dir.y < 0.0) {
		sky = mix(horizon_color, ground_color, clamp(-dir.y * 4.0, 0.0, 1.0));
	}

	vec3 sun_dir = LIGHT0_DIRECTION;
	float sun_dot = max(dot(dir, sun_dir), 0.0);
	vec3 sun_light = LIGHT0_COLOR * LIGHT0_ENERGY;
	// The sun and a faint warm glow around it, hidden as the sky clouds over. Not in the cubemap
	// that lights the world: the sun light does that already.
	float shown = AT_CUBEMAP_PASS ? 0.0 : 1.0 - cloud_cover * 0.8;
	sky += sun_light * pow(sun_dot, 24.0) * 0.08 * shown;
	float disc = smoothstep(cos(sun_size), cos(sun_size * 0.7), sun_dot) * shown;

	float clouds = 0.0;
	vec3 cloud = cloud_color;
	if (dir.y > 0.01) {
		// The cloud layer as a plane overhead: its pattern shrinks towards the horizon.
		vec2 uv = dir.xz / (dir.y + 0.06) * 0.5 + cloud_offset;
		// Few octaves: rounded, puffy shapes without wispy detail.
		float shape = fbm(uv, 3);
		// More cover lowers the threshold; a narrow edge keeps the clouds' outlines clean.
		float threshold = mix(0.72, 0.32, cloud_cover);
		clouds = smoothstep(threshold, threshold + 0.02, shape);
		// Thick cloud, and cloud away from the sun, is shaded.
		float thickness = smoothstep(threshold, threshold + 0.3, shape);
		vec3 shade = mix(cloud_shade_color, cloud_shade_color * 0.75, cloud_darkness);
		cloud = mix(cloud_color, shade, thickness * (0.55 + 0.45 * (1.0 - sun_dot)));
		clouds *= smoothstep(0.01, 0.16, dir.y);
	}

	vec3 color = mix(sky, cloud, clouds);
	color += sun_light * disc * 1.5 * (1.0 - clouds);
	COLOR = color;
}
