shader_type spatial;
// Terrain (R45): ground materials blended by land cover, slope and height — grass, forest
// floor, soil on farmland, rock on steep slopes and grey ground, snow up high. The land cover
// arrives as a vertex colour from OpenStreetMap (alpha marks water) and tints the textures,
// so meadows, fields and forests keep their character. CC0 textures from ambientCG (credits:
// docs/CREDITS.md).

uniform sampler2D grass_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D grass_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D forest_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D forest_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D soil_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D soil_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D rock_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D rock_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D snow_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D snow_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform float snow_line = 2400.0;
// 0 plain colours (Low), 1 textures, 2 textures with a second tiling scale and triplanar rock.
uniform int detail = 2;
// Beyond this the ground is its average colour: textures would only shimmer.
uniform float detail_distance = 450.0;

varying vec3 world_position;
varying vec3 world_normal;

float hash(vec2 p) {
	return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float value_noise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(
		mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
		mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x),
		u.y
	);
}

void vertex() {
	// Degenerate triangles at chunk edges can leave a vertex without a normal; lit with it,
	// a pixel turned NaN.
	if (dot(NORMAL, NORMAL) < 1e-12) {
		NORMAL = vec3(0.0, 1.0, 0.0);
	}
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	world_normal = normalize((MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz);
}

// A texture laid flat on the ground, `tile` metres per repeat; on the highest detail mixed
// with itself at a larger scale so the tiling does not show.
vec4 ground(sampler2D albedo, vec2 position, float tile) {
	vec4 near = texture(albedo, position / tile);
	if (detail < 2) {
		return near;
	}
	vec4 far = texture(albedo, position / (tile * 4.7) + vec2(0.37, 0.71));
	return mix(near, far, 0.35);
}

// A texture's colour relative to its average (its smallest mip), to tint it with the cover.
vec3 relative(sampler2D albedo, vec4 sample) {
	return sample.rgb / max(textureLod(albedo, vec2(0.5), 12.0).rgb, vec3(0.05));
}

// Rock on cliffs: projected along the three axes and blended by the slope, so it is not
// stretched where the ground is steep.
vec4 triplanar(sampler2D albedo, vec3 position, vec3 normal, float tile) {
	if (detail < 2) {
		return texture(albedo, position.xz / tile);
	}
	vec3 weights = pow(abs(normal), vec3(4.0));
	weights /= weights.x + weights.y + weights.z;
	return texture(albedo, position.zy / tile) * weights.x
		+ texture(albedo, position.xz / tile) * weights.y
		+ texture(albedo, position.xy / tile) * weights.z;
}

void fragment() {
	// Large patches break up flat areas; land cover colours arrive in sRGB.
	float patches = value_noise(world_position.xz / 90.0) * 0.65 + value_noise(world_position.xz / 9.0) * 0.35;
	vec3 cover = pow(COLOR.rgb, vec3(2.2));
	float steepness = 1.0 - world_normal.y;

	// Which ground, from the land cover's colour: dark is forest, yellowish is farmland, grey
	// is rock or built-up; steep slopes are rock whatever the cover, high ground is snow.
	float brightness = dot(COLOR.rgb, vec3(0.333));
	float saturation = max(COLOR.r, max(COLOR.g, COLOR.b)) - min(COLOR.r, min(COLOR.g, COLOR.b));
	float forest = smoothstep(0.24, 0.18, brightness);
	float soil = smoothstep(-0.06, 0.01, COLOR.r - COLOR.g) * (1.0 - forest);
	float rock = max(
		smoothstep(0.16, 0.30, steepness + patches * 0.05),
		smoothstep(0.12, 0.06, saturation) * smoothstep(0.35, 0.42, brightness)
	);
	float snow = smoothstep(snow_line - 150.0, snow_line + 150.0, world_position.y + patches * 120.0)
		* smoothstep(0.45, 0.25, steepness);

	vec3 plain = cover * (0.85 + patches * 0.3);
	plain = mix(plain, vec3(0.45, 0.43, 0.40), rock);
	plain = mix(plain, vec3(0.93, 0.95, 0.97), snow);
	vec3 color = plain;
	vec3 normal_map = vec3(0.5, 0.5, 1.0);

	float distance = length(world_position - CAMERA_POSITION_WORLD);
	float near = detail > 0 ? 1.0 - smoothstep(detail_distance * 0.6, detail_distance, distance) : 0.0;
	if (near > 0.0) {
		vec2 xz = world_position.xz;
		vec3 textured = cover * relative(grass_albedo, ground(grass_albedo, xz, 3.0));
		textured = mix(textured, cover * relative(forest_albedo, ground(forest_albedo, xz, 2.5)), forest);
		textured = mix(textured, cover * relative(soil_albedo, ground(soil_albedo, xz, 3.5)) * 1.1, soil);
		textured *= 0.9 + patches * 0.2;
		textured = mix(textured, triplanar(rock_albedo, world_position, world_normal, 6.0).rgb, rock);
		textured = mix(textured, ground(snow_albedo, xz, 4.0).rgb, snow);
		color = mix(plain, textured, near);

		vec3 n = texture(grass_normal, xz / 3.0).rgb;
		n = mix(n, texture(forest_normal, xz / 2.5).rgb, forest);
		n = mix(n, texture(soil_normal, xz / 3.5).rgb, soil);
		n = mix(n, texture(rock_normal, xz / 6.0).rgb, rock);
		n = mix(n, texture(snow_normal, xz / 4.0).rgb, snow);
		normal_map = mix(normal_map, n, near);
	}

	// Under lakes and rivers: the bed, mostly hidden by the water surface above it.
	float water = smoothstep(0.4, 0.6, COLOR.a);
	ALBEDO = mix(color, vec3(0.08, 0.1, 0.09), water);
	NORMAL_MAP = normal_map;
	NORMAL_MAP_DEPTH = 0.8;
	ROUGHNESS = mix(0.95, 0.75, rock) - snow * 0.25;
	SPECULAR = 0.2 + snow * 0.15;
}
