shader_type spatial;
render_mode cull_disabled;
// All roads (R119): Rust supplies material, width and marking style in mesh COLOR.
// Textured asphalt is slightly darker and smoother where wheels
// run, white markings worn by traffic — solid edge lines and a dashed centre line, anti-aliased
// so they hold in the distance — and its skirts below the edges: a narrow, ragged strip of
// gravel the asphalt frays into, then the verge's grass, mapped like the ground's so the two
// meet without a seam. Wet in rain: darker, glossier, with puddles. UV.x runs 0–1 across the
// road (beyond, the skirts), UV.y is the distance along the route in metres.

#include "res://shaders/ground.gdshaderinc"
#include "res://shaders/winter.gdshaderinc"
#include "res://shaders/autumn_litter.gdshaderinc"
#include "res://shaders/bridge_surface.gdshaderinc"

uniform sampler2D asphalt_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D asphalt_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D asphalt_rough : hint_roughness_r, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D gravel_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D gravel_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D grass_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D grass_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D dirt_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D dirt_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D concrete_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D concrete_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D paving_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D paving_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform vec3 marking_color : source_color = vec3(0.85, 0.85, 0.82);
uniform bool shoulders = true;
uniform float line_width = 0.12;
uniform float dash_length = 3.0;
uniform float dash_gap = 6.0;
uniform float asphalt_tile = 2.5;
uniform float gravel_tile = 1.5;
// 0 dry, 1 soaked: darker asphalt with a sheen.
uniform float wetness : hint_range(0.0, 1.0) = 0.0;
uniform bool winter_exposure_mask = false;

#include "res://shaders/puddles.gdshaderinc"

varying vec3 world_position;
varying vec3 world_normal;
varying vec2 world_xz;

void vertex() {
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	world_normal = normalize((MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz);
	world_xz = world_position.xz;
}

// 1 inside [from, to] across the road, with edges one pixel soft.
float band(float across, float from, float to) {
	float soft = max(fwidth(across), 1e-4);
	return smoothstep(from - soft, from + soft, across) * (1.0 - smoothstep(to - soft, to + soft, across));
}

void fragment() {
	float material = round(COLOR.r * 8.0);
	float road_width = max(COLOR.g * 32.0, 1.0);
	float paint = round(COLOR.b * 4.0);
	float flags = round(COLOR.a * 8.0);
	float lanes = max(mod(flags, 4.0), 1.0);
	float across = UV.x * road_width;
	float along = UV.y;
	// Metres across and along, wrapped along so tens of kilometres keep their precision.
	vec2 metres = vec2(across, mod(along, 256.0 * asphalt_tile * gravel_tile));

	vec2 asphalt_uv = ground_tiles(world_xz, asphalt_tile);
	vec3 asphalt = texture(asphalt_albedo, asphalt_uv).rgb;
	vec3 asphalt_detail = ground_unpack(texture(asphalt_normal, asphalt_uv));
	float asphalt_roughness = texture(asphalt_rough, asphalt_uv).r;
	vec2 gravel_uv = ground_tiles(world_xz, gravel_tile);
	vec3 gravel = texture(gravel_albedo, gravel_uv).rgb;
	vec3 gravel_detail = ground_unpack(texture(gravel_normal, gravel_uv));
	if (material > 0.5 && material < 1.5) {
		asphalt = gravel;
		asphalt_detail = gravel_detail;
		asphalt_roughness = 0.95;
	} else if (material > 1.5 && material < 2.5) {
		asphalt = texture(dirt_albedo, ground_tiles(world_xz, 2.0)).rgb;
		asphalt_detail = ground_unpack(texture(dirt_normal, ground_tiles(world_xz, 2.0)));
		asphalt_roughness = 0.96;
	} else if (material > 2.5 && material < 3.5) {
		asphalt = texture(concrete_albedo, ground_tiles(world_xz, 2.0)).rgb;
		asphalt_detail = ground_unpack(texture(concrete_normal, ground_tiles(world_xz, 2.0)));
		asphalt_roughness = 0.88;
	} else if (material > 3.5 && material < 5.5) {
		float scale = material > 4.5 ? 1.4 : 2.0;
		asphalt = texture(paving_albedo, ground_tiles(world_xz, scale)).rgb;
		asphalt_detail = ground_unpack(texture(paving_normal, ground_tiles(world_xz, scale)));
		asphalt_roughness = 0.92;
	}
	// Wheel tracks: darker and polished, a metre and a half in from either edge.
	float track = band(across, 1.0, 2.1) + band(across, road_width - 2.1, road_width - 1.0);
	track *= 0.5 + 0.5 * ground_noise(vec2(across * 2.0, along / 7.0));
	track *= material < 0.5 ? 1.0 : 0.0;
	asphalt *= 1.0 - 0.12 * track;
	asphalt_roughness -= 0.1 * track;

	// Markings, worn away in flecks and patches.
	float edge = paint == 2.0 ? 1.0 : 0.25;
	float edges = band(across, edge, edge + line_width)
		+ band(across, road_width - edge - line_width, road_width - edge);
	float half_line = line_width * 0.5;
	float centre_line = 0.0;
	for (int lane = 1; lane < 4; lane++) {
		if (float(lane) < lanes && paint < 2.5) {
			float at = edge + (road_width - 2.0 * edge) * float(lane) / lanes;
			centre_line += band(across, at - half_line, at + half_line);
		}
	}
	float length = paint == 2.0 ? 6.0 : dash_length;
	float gap = paint == 2.0 ? 12.0 : dash_gap;
	float dash = mod(along, length + gap);
	float soft = max(fwidth(along), 1e-4);
	centre_line *= 1.0 - smoothstep(length - soft, length + soft, dash);
	float wear = smoothstep(0.25, 0.55, dot(asphalt, vec3(0.6)) + ground_noise(vec2(across * 3.0, along * 0.6)) * 0.5);
	float marking = clamp(edges + centre_line, 0.0, 1.0) * mix(0.55, 1.0, wear);
	marking *= paint > 0.5 ? 1.0 : 0.0;
	vec3 albedo = mix(asphalt, marking_color, marking);
	float roughness = mix(asphalt_roughness, 0.6, marking);

	// Gravel on the skirts; the asphalt's edge frays into it.
	float outside = max(-UV.x, UV.x - 1.0) * road_width;
	float fray = (ground_noise(vec2(along * 1.5, UV.x > 0.5 ? 7.0 : 3.0)) - 0.5) * 0.12;
	float shoulder = smoothstep(-0.02, 0.02, outside + fray);
	shoulder *= shoulders ? 1.0 : 0.0;
	albedo = mix(albedo, gravel * 0.85, shoulder);
	roughness = mix(roughness, 0.95, shoulder);
	vec3 detail = normalize(mix(asphalt_detail, gravel_detail, shoulder));
	vec3 normal = ground_bend(world_normal, detail, vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), 1.0);
	// Beyond half a metre or so the verge is grass, its edge ragged.
	vec2 grass_uv = ground_tiles(world_xz, 3.0);
	float ragged = (ground_noise(vec2(along * 0.9, UV.x > 0.5 ? 11.0 : 5.0)) - 0.5) * 0.5;
	float verge = smoothstep(0.35, 0.7, outside + ragged);
	verge *= shoulders ? 1.0 : 0.0;
	float middle = flags > 3.5 ? 1.0 - smoothstep(0.1, 0.16, abs(UV.x - 0.5) + fray * 0.5) : 0.0;
	if (material > 5.5) { middle = 1.0; }
	verge = max(verge, middle);
	vec3 grass_bent = ground_bend(world_normal, ground_unpack(texture(grass_normal, grass_uv)),
		vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), 0.8);
	albedo = mix(albedo, texture(grass_albedo, grass_uv).rgb * vec3(0.34, 0.44, 0.27), verge);
	normal = normalize(mix(normal, grass_bent, verge));
	// Settled street snow, worn into paired wheel grooves in each traffic lane.
	float lane_width = road_width / lanes;
	float lane_x = mod(across, lane_width) - lane_width * 0.5;
	float wander = sin(along * 0.09) * 0.045 + sin(along * 0.027) * 0.07;
	float axle_half = min(0.72, lane_width * 0.26);
	float tyre = abs(abs(lane_x + wander) - axle_half);
	float groove = 1.0 - smoothstep(0.09, 0.18 + fwidth(across), tyre);
	float settled = clamp((world_position.y - winter_snow_line) / 1800.0, 0.0, 1.0);
	float patches = ground_noise(vec2(across * 2.7, along * 0.21));
	float street_snow = smoothstep(0.22 - settled * 0.18, 0.78 - settled * 0.28, patches);
	street_snow = mix(street_snow, 0.96, smoothstep(lane_width * 0.32, lane_width * 0.49, abs(lane_x)));
	street_snow *= 1.0 - groove * 0.95;
	float snow = winter_cover(world_position, world_normal) * mix(street_snow, 1.0, verge);
	snow *= winter_exposure_mask ? UV2.x : 1.0;
	albedo = mix(albedo, winter_snow_color.rgb, snow);
	normal = normalize(mix(normal, world_normal, snow * 0.7));
	roughness = mix(roughness, 0.8, snow);
	roughness = mix(roughness, 0.92, verge);

	// Rain darkens and smooths the asphalt, gravel less.
	float sealed = material < 0.5 || (material > 2.5 && material < 5.5) ? 1.0 : 0.0;
	float wet = wetness * mix(0.5, 1.0, sealed) * mix(1.0, 0.5, shoulder) * (1.0 - verge);
	albedo *= 1.0 - 0.4 * wet;
	roughness = mix(roughness, mix(0.65, 0.25, sealed), wet);
	float water = puddle(world_xz, world_normal, clamp(abs(UV.x - 0.5) * 2.0, 0.0, 1.0)) * (1.0 - shoulder) * (1.0 - verge);
	ALBEDO = mix(albedo, puddle_color * 0.6, water * 0.6);
	ROUGHNESS = mix(roughness, 0.08, water);
	if (FRONT_FACING && world_normal.y > 0.1 && COLOR.a > 0.01) {
		autumn_litter(world_position, clamp(abs(UV.x - 0.5) * 2.0, 0.0, 1.0), marking, wetness, ALBEDO, ROUGHNESS);
	}
	SPECULAR = mix(0.4, 0.5, max(wet, water));
	NORMAL = normalize((VIEW_MATRIX * vec4(normalize(mix(normal, world_normal, water)), 0.0)).xyz);
	float structure = 1.0 - step(0.01, COLOR.a);
	float steel = structure * (1.0 - step(0.025, distance(COLOR.rgb, steel_srgb)));
	if (!FRONT_FACING || world_normal.y < 0.1 || structure > 0.5) {
		// Markings and puddles belong to the wearing surface, not its exposed underside.
		vec3 facing = FRONT_FACING ? world_normal : -world_normal;
		bridge_concrete(world_position, facing, albedo, normal, roughness);
		ALBEDO = mix(albedo, steel_color, steel) * (1.0 - wetness * 0.22);
		ROUGHNESS = mix(roughness, 0.45, steel);
		METALLIC = 0.65 * steel;
		NORMAL = normalize((VIEW_MATRIX * vec4(mix(normal, facing, steel), 0.0)).xyz);
	}
}
