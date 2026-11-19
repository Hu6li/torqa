// The sky (R89, ADR 0020): a physically based atmosphere — Rayleigh and Mie scattering of the
// sun's light along the view ray, so the sky is deep blue overhead, paler at the horizon, and
// warm at sunrise and sunset on its own — with the sun's disc and soft, sunlit clouds of
// fractal noise overhead, fading into the haze at the horizon. Overcast weather greys the
// atmosphere and closes the cloud cover.
//
// Drawn in three passes, as each part needs (a tablet's GPU spent ~10 ms a frame on the sky at
// full resolution): the smooth atmosphere at a quarter of the resolution, the clouds over it at
// half, and only the sun's disc and the haze at full. The engine would fog each pass again, so
// the haze is the shader's own, applied once.
shader_type sky;
render_mode use_half_res_pass, use_quarter_res_pass, disable_fog;

// Clouds' sunlit and shaded colours, and the overcast sky's (palette `sky.*`).
uniform vec3 cloud_color : source_color = vec3(0.957, 0.961, 0.969);
uniform vec3 cloud_shade_color : source_color = vec3(0.604, 0.639, 0.678);
uniform vec3 overcast_top : source_color = vec3(0.549, 0.576, 0.608);
uniform vec3 overcast_horizon : source_color = vec3(0.706, 0.725, 0.749);
uniform vec3 ground_color : source_color = vec3(0.3, 0.33, 0.25);
// Share of the sky covered (0 clear, 1 overcast) and how grey the weather is.
uniform float cloud_cover : hint_range(0.0, 1.0) = 0.35;
uniform float cloud_darkness : hint_range(0.0, 1.0) = 0.0;
// How far the clouds have drifted, in cloud-layer units (set by the world, slowly). A global:
// changing a material's own uniform every frame would redraw the sky's lighting every frame.
global uniform vec2 cloud_offset;
// Brightness of the scattered light; the sun's intensity in the scattering model.
uniform float sky_energy = 1.0;
uniform float sun_size : hint_range(0.0, 0.1) = 0.012;
// The environment's fog as it falls on the sky (its light colour, aerial perspective, sun
// scatter and sky affect), copied by the world.
uniform vec3 haze_color : source_color = vec3(0.8, 0.85, 0.9);
uniform float haze_aerial = 0.6;
uniform float haze_sun_scatter = 0.0;
uniform float haze_sky = 0.25;

const float PLANET = 6360e3;
const float ATMOSPHERE = 6420e3;
const vec3 RAYLEIGH = vec3(5.5e-6, 13.0e-6, 22.4e-6);
const float MIE = 21e-6;
const float RAYLEIGH_HEIGHT = 7994.0;
const float MIE_HEIGHT = 1200.0;
const float MIE_G = 0.76;
const float SUN_INTENSITY = 20.0;

// A number in [0, 1) for the lattice point `p`: an integer hash, as for the ground — the old
// fract-based one lost its precision across the cloud plane and drew flat-edged bands.
float hash(vec2 p) {
	uvec2 v = uvec2(ivec2(p)) * 1664525u + 1013904223u;
	v.x += v.y * 1664525u;
	v.y += v.x * 1664525u;
	v ^= v >> 16u;
	v.x += v.y * 1664525u;
	return float(v.x >> 8u) / 16777216.0;
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

// Octaves turned against each other, so the noise's square lattice never lines up into
// streaks when the cloud plane is seen at a low angle.
const mat2 TURN = mat2(vec2(0.8, 0.6), vec2(-0.6, 0.8));

float fbm(vec2 p, int octaves) {
	float value = 0.0;
	float amplitude = 0.5;
	for (int i = 0; i < octaves; i++) {
		value += amplitude * noise(p);
		p = TURN * p * 2.03 + vec2(17.1, 9.2);
		amplitude *= 0.5;
	}
	return value;
}

// Distance along `dir` from `origin` to where it leaves a sphere of `radius` round the planet's
// centre (the origin is inside).
float to_sphere(vec3 origin, vec3 dir, float radius) {
	float b = dot(origin, dir);
	float c = dot(origin, origin) - radius * radius;
	return -b + sqrt(max(b * b - c, 0.0));
}

// Optical depth (Rayleigh, Mie) from `origin` towards the sun.
vec2 towards_sun(vec3 origin, vec3 sun) {
	float length_to_top = to_sphere(origin, sun, ATMOSPHERE);
	float step_length = length_to_top / 4.0;
	vec2 depth = vec2(0.0);
	for (int i = 0; i < 4; i++) {
		vec3 point = origin + sun * (float(i) + 0.5) * step_length;
		float height = length(point) - PLANET;
		if (height < 0.0) {
			return vec2(1e9);
		}
		depth += vec2(exp(-height / RAYLEIGH_HEIGHT), exp(-height / MIE_HEIGHT)) * step_length;
	}
	return depth;
}

// The light the atmosphere scatters towards the eye along `dir`.
vec3 atmosphere(vec3 dir, vec3 sun) {
	vec3 origin = vec3(0.0, PLANET + 500.0, 0.0);
	float length_to_top = to_sphere(origin, dir, ATMOSPHERE);
	float step_length = length_to_top / 12.0;
	vec2 depth = vec2(0.0);
	vec3 rayleigh = vec3(0.0);
	vec3 mie = vec3(0.0);
	for (int i = 0; i < 12; i++) {
		vec3 point = origin + dir * (float(i) + 0.5) * step_length;
		float height = length(point) - PLANET;
		vec2 here = vec2(exp(-height / RAYLEIGH_HEIGHT), exp(-height / MIE_HEIGHT)) * step_length;
		depth += here;
		vec2 to_sun = towards_sun(point, sun);
		vec3 through = exp(-(RAYLEIGH * (depth.x + to_sun.x) + MIE * 1.1 * (depth.y + to_sun.y)));
		rayleigh += through * here.x;
		mie += through * here.y;
	}
	float mu = dot(dir, sun);
	float rayleigh_phase = 3.0 / (16.0 * PI) * (1.0 + mu * mu);
	float g2 = MIE_G * MIE_G;
	float mie_phase = 3.0 / (8.0 * PI) * ((1.0 - g2) * (1.0 + mu * mu))
		/ ((2.0 + g2) * pow(1.0 + g2 - 2.0 * MIE_G * mu, 1.5));
	return SUN_INTENSITY * (rayleigh * RAYLEIGH * rayleigh_phase + mie * MIE * mie_phase);
}

// The sun's light after crossing the atmosphere to the eye: reddened when low.
vec3 sunlight(vec3 sun) {
	vec2 depth = towards_sun(vec3(0.0, PLANET + 500.0, 0.0), sun);
	return exp(-(RAYLEIGH * depth.x + MIE * 1.1 * depth.y));
}

// Atmosphere, overcast and, below the horizon, the ground: everything but clouds and sun.
vec3 backdrop(vec3 dir, vec3 sun) {
	// Below the horizon the atmosphere is seen just above it, then the ground's colour.
	vec3 look = normalize(vec3(dir.x, max(dir.y, 0.0) + 0.002, dir.z));
	vec3 air = atmosphere(look, sun) * sky_energy;
	vec3 overcast = mix(overcast_horizon, overcast_top, pow(max(dir.y, 0.0), 0.5)) * sky_energy;
	vec3 colour = mix(air, overcast, cloud_darkness);
	if (dir.y < 0.0) {
		colour = mix(colour, ground_color * sky_energy * 0.5, clamp(-dir.y * 6.0, 0.0, 1.0));
	}
	return colour;
}

// Where the cloud layer is seen along `dir` (overhead only), wrapped so the hash keeps its
// precision however far the clouds drifted. Its pattern shrinks towards the horizon. The pattern
// jumps where it wraps: centred half a wrap away, that line stays out of the sky until the clouds
// have drifted for a day and more, not overhead from the start (#560).
vec2 cloud_uv(vec3 dir) {
	return mod(dir.xz / (dir.y + 0.08) * 0.35 + cloud_offset + 512.0, 1024.0);
}

// How much cloud covers the sky along `dir`, from the layer's density there.
float cloud_amount(vec3 dir, float density) {
	float threshold = mix(0.68, 0.22, cloud_cover);
	// Far clouds sink into the haze.
	return smoothstep(threshold, threshold + 0.25, density) * smoothstep(0.0, 0.18, dir.y);
}

// `colour` with the sunlit clouds over it.
vec3 with_clouds(vec3 dir, vec3 sun, vec3 colour) {
	if (dir.y <= 0.0) {
		return colour;
	}
	vec3 sun_colour = sunlight(sun);
	vec2 uv = cloud_uv(dir);
	float density = fbm(uv, 6);
	float threshold = mix(0.68, 0.22, cloud_cover);
	float clouds = smoothstep(threshold, threshold + 0.25, density);
	// Light through the cloud: thinner towards the sun means brighter.
	float towards = fbm(uv + sun.xz * 0.06, 6);
	float lit = clamp(0.75 - (towards - density) * 2.5 - (density - threshold) * 0.8, 0.0, 1.0);
	vec3 shade = mix(cloud_shade_color, cloud_shade_color * 0.55, cloud_darkness);
	vec3 bright = cloud_color * mix(sun_colour, vec3(0.8), cloud_darkness);
	vec3 cloud = mix(shade, bright, lit) * sky_energy;
	// A silver lining near the sun.
	float near_sun = pow(max(dot(dir, sun), 0.0), 8.0);
	cloud += sun_colour * near_sun * (1.0 - clouds) * 0.6 * sky_energy * (1.0 - cloud_darkness);
	return mix(colour, cloud, cloud_amount(dir, density));
}

void sky() {
	vec3 dir = EYEDIR;
	vec3 sun = LIGHT0_DIRECTION;
	if (AT_QUARTER_RES_PASS) {
		COLOR = backdrop(dir, sun);
	} else if (AT_HALF_RES_PASS) {
		COLOR = with_clouds(dir, sun, QUARTER_RES_COLOR.rgb);
	} else {
		vec3 colour = HALF_RES_COLOR.rgb;
		// The sun's disc and the haze; not in the cubemap that lights the world, where the sun
		// light does the one and the engine leaves out the other.
		if (!AT_CUBEMAP_PASS) {
			float mu = dot(dir, sun);
			if (mu > cos(sun_size)) {
				float clouds = dir.y > 0.0 ? cloud_amount(dir, fbm(cloud_uv(dir), 6)) : 0.0;
				float disc = smoothstep(cos(sun_size), cos(sun_size * 0.8), mu);
				colour += sunlight(sun) * disc * 40.0 * (1.0 - clouds) * (1.0 - cloud_darkness);
			}
			vec3 haze = mix(haze_color, colour, haze_aerial);
			if (haze_sun_scatter > 0.001) {
				haze += LIGHT0_COLOR * LIGHT0_ENERGY * pow(max(mu, 0.0), 8.0) * PI * haze_sun_scatter;
			}
			colour = mix(colour, haze, haze_sky);
		}
		COLOR = colour;
	}
}
