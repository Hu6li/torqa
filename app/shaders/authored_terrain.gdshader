// Ground of a world file with ORQWA's terrain layers (`ORQWA_terrain_layers`, ADR 0042): the
// author's base colour, often an aerial photo, carries the ground's look from afar; near the
// camera up to eight tiling layers add the detail a photo cannot, each weighted by the author's
// masks and tiled in the world's horizontal plane, or on steep ground, cliffs and banks, from
// the side too, so it never stretches. A layer's albedo is divided by its own average colour,
// so it changes texture, not tone. The world's light is its own (R268): no
// seasons, snow or wetness here.
shader_type spatial;
render_mode cull_back;

uniform vec4 base_color : source_color = vec4(1.0);
uniform sampler2D base_texture : source_color, filter_linear_mipmap_anisotropic, repeat_disable;
uniform bool has_base_texture = false;
uniform float base_roughness : hint_range(0.0, 1.0) = 1.0;

uniform sampler2D weights_0 : hint_default_black, filter_linear_mipmap, repeat_disable;
uniform sampler2D weights_1 : hint_default_black, filter_linear_mipmap, repeat_disable;
uniform int layer_count = 0;
uniform sampler2D layer_albedo[8] : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D layer_normal[8] : filter_linear_mipmap_anisotropic, repeat_enable;
uniform float layer_has_normal[8];
uniform float layer_size[8];
uniform float layer_roughness[8];
uniform vec3 layer_mean[8];
uniform float fade_near = 40.0;
uniform float fade_far = 160.0;
uniform float strength : hint_range(0.0, 1.0) = 1.0;

varying vec3 world_position;
varying vec3 world_normal;

// How much of each projection a surface takes: from above (y), from the east (x), from the
// south (z); steep faces leave the projection from above.
vec3 projections(vec3 n) {
	vec3 w = pow(abs(n), vec3(4.0));
	return w / (w.x + w.y + w.z);
}

vec4 layer_sample(sampler2D map, vec3 at, vec3 w) {
	vec4 colour = texture(map, at.xz) * w.y;
	if (w.x > 0.01) {
		colour += texture(map, at.zy) * w.x;
	}
	if (w.z > 0.01) {
		colour += texture(map, at.xy) * w.z;
	}
	return colour;
}

void vertex() {
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	world_normal = normalize((MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz);
}

void fragment() {
	vec4 base = base_color;
	if (has_base_texture) {
		base *= texture(base_texture, UV);
	}
	ALBEDO = base.rgb;
	ROUGHNESS = base_roughness;
	float near = (1.0 - smoothstep(fade_near, fade_far, length(VERTEX))) * strength;
	if (near > 0.001 && layer_count > 0) {
		vec4 first = texture(weights_0, UV);
		vec4 second = layer_count > 4 ? texture(weights_1, UV) : vec4(0.0);
		float weights[8] = float[8](
			first.r, first.g, first.b, first.a, second.r, second.g, second.b, second.a
		);
		vec3 detail = vec3(0.0);
		vec3 bumps = vec3(0.0);
		float rough = 0.0;
		float total = 0.0;
		vec3 w = projections(normalize(world_normal));
		for (int i = 0; i < 8; i++) {
			if (i >= layer_count || weights[i] < 0.002) {
				continue;
			}
			vec3 at = world_position / layer_size[i];
			detail += weights[i] * layer_sample(layer_albedo[i], at, w).rgb / layer_mean[i];
			// Bumps from above only: on cliffs the photo and the side projections carry the
			// detail.
			vec3 bump = layer_has_normal[i] > 0.5
				? mix(vec3(0.0, 0.0, 1.0), texture(layer_normal[i], at.xz).rgb * 2.0 - 1.0, w.y)
				: vec3(0.0, 0.0, 1.0);
			bumps += weights[i] * bump;
			rough += weights[i] * layer_roughness[i];
			total += weights[i];
		}
		if (total > 0.002) {
			float amount = near * clamp(total, 0.0, 1.0);
			ALBEDO = base.rgb * mix(vec3(1.0), detail / total, amount);
			ROUGHNESS = mix(base_roughness, rough / total, amount);
			// Layers lie in the horizontal plane: east along x, north along −z.
			vec3 n = normalize(world_normal);
			vec3 east = normalize(vec3(1.0, 0.0, 0.0) - n * n.x);
			vec3 north = cross(east, n);
			vec3 b = normalize(bumps / total);
			vec3 bent = normalize(east * b.x + north * b.y + n * b.z);
			NORMAL = normalize((VIEW_MATRIX * vec4(mix(n, bent, amount), 0.0)).xyz);
		}
	}
}
