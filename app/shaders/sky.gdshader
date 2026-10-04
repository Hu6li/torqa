// The sky (R45): a gradient from the horizon up, the sun with its halo, and a layer of clouds
// drifting overhead — fractal noise lit towards the sun, more detailed on higher graphics
// presets — fading into the haze at the horizon.
shader_type sky;

uniform vec3 top_color : source_color = vec3(0.32, 0.52, 0.82);
uniform vec3 horizon_color : source_color = vec3(0.68, 0.78, 0.88);
uniform vec3 ground_color : source_color = vec3(0.25, 0.3, 0.32);
// Share of the sky covered (0 clear, 1 overcast) and how dark the clouds' undersides are.
uniform float cloud_cover : hint_range(0.0, 1.0) = 0.35;
uniform float cloud_darkness : hint_range(0.0, 1.0) = 0.35;
// Noise octaves for the clouds' shapes and steps towards the sun for their shading.
uniform int cloud_octaves : hint_range(1, 8) = 5;
uniform int light_steps : hint_range(0, 6) = 2;
// How far the clouds have drifted, in cloud-layer units (set by the world, slowly).
uniform vec2 cloud_offset = vec2(0.0);
uniform float sun_size : hint_range(0.0, 0.1) = 0.012;

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

float density(vec2 p, int octaves) {
	float shape = fbm(p, octaves);
	// More cover lowers the threshold; the soft edge keeps the clouds fluffy.
	float threshold = mix(0.75, 0.3, cloud_cover);
	return smoothstep(threshold, threshold + 0.25, shape);
}

void sky() {
	vec3 dir = EYEDIR;
	float up = max(dir.y, 0.0);
	vec3 sky = mix(horizon_color, top_color, pow(up, 0.55));
	if (dir.y < 0.0) {
		sky = mix(horizon_color, ground_color, clamp(-dir.y * 4.0, 0.0, 1.0));
	}

	vec3 sun_dir = LIGHT0_DIRECTION;
	float sun_dot = max(dot(dir, sun_dir), 0.0);
	vec3 sun_light = LIGHT0_COLOR * LIGHT0_ENERGY;
	// A halo around the sun and the disc itself, hidden as the sky clouds over. Not in the
	// cubemap that lights the world: the sun light does that already, and its bright disc
	// there showed as glowing spots in the haze.
	float halo = AT_CUBEMAP_PASS ? 0.0 : 1.0;
	sky += sun_light * pow(sun_dot, 48.0) * 0.18 * (1.0 - cloud_cover * 0.8) * halo;
	float disc = smoothstep(cos(sun_size), cos(sun_size * 0.6), sun_dot) * halo;

	float clouds = 0.0;
	vec3 cloud_color = vec3(0.0);
	if (dir.y > 0.01) {
		// The cloud layer as a plane overhead: its texture shrinks towards the horizon.
		vec2 uv = dir.xz / (dir.y + 0.06) * 0.9 + cloud_offset;
		// Reflections and ambient light need no fine detail.
		int octaves = AT_CUBEMAP_PASS ? min(cloud_octaves, 3) : cloud_octaves;
		clouds = density(uv, octaves);
		// Shading: thicker cloud towards the sun means a darker underside.
		float shadow = 0.0;
		int steps = AT_CUBEMAP_PASS ? 0 : light_steps;
		vec2 towards_sun = normalize(sun_dir.xz + vec2(1e-4)) * 0.08;
		for (int i = 1; i <= steps; i++) {
			shadow += density(uv + towards_sun * float(i), max(octaves - 2, 1));
		}
		float lit = steps > 0 ? exp(-shadow / float(steps) * 2.2) : 1.0 - clouds * 0.5;
		vec3 bright = mix(vec3(1.0), sun_light, 0.35);
		vec3 dark = mix(horizon_color, vec3(0.45, 0.48, 0.52), 0.6);
		cloud_color = mix(dark * (1.0 - cloud_darkness), bright, lit);
		// Clouds near the sun glow at their edges.
		cloud_color += sun_light * pow(sun_dot, 12.0) * (1.0 - clouds) * 0.6;
		// Fade into the haze at the horizon.
		clouds *= smoothstep(0.01, 0.18, dir.y);
	}

	vec3 color = mix(sky, cloud_color, clouds);
	color += sun_light * disc * 6.0 * (1.0 - clouds) * (1.0 - cloud_cover);
	COLOR = color;
}
