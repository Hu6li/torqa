// Streets, tracks and paths of the map around the route (R45, ADR 0011): flat palette colours —
// blue-grey asphalt for paved streets, sandy gravel for tracks and paths, with a strip of grass
// down the middle of tracks — faceted like the ground, wet with puddles in rain. UV.x runs 0–1
// across, UV.y is the distance along in metres.
shader_type spatial;

uniform vec3 surface_color : source_color = vec3(0.557, 0.584, 0.608);
uniform vec3 grass_color : source_color = vec3(0.698, 0.757, 0.471);
// 1 for tracks: grass in the middle where wheels do not run.
uniform float middle_grass : hint_range(0.0, 1.0) = 0.0;
uniform float wetness : hint_range(0.0, 1.0) = 0.0;

#include "res://shaders/puddles.gdshaderinc"

varying vec2 world_xz;

void vertex() {
	world_xz = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xz;
}

void fragment() {
	NORMAL = normalize(cross(dFdy(VERTEX), dFdx(VERTEX)));
	float off_centre = abs(UV.x - 0.5);
	float soft = max(fwidth(off_centre), 1e-4);
	float middle = middle_grass * (1.0 - smoothstep(0.12 - soft, 0.12 + soft, off_centre));
	vec3 surface = surface_color * (1.0 - wetness * 0.3);
	vec3 albedo = mix(surface, grass_color, middle);
	vec3 world_normal = (INV_VIEW_MATRIX * vec4(NORMAL, 0.0)).xyz;
	float water = puddle(world_xz, world_normal, clamp(abs(UV.x - 0.5) * 2.0, 0.0, 1.0)) * (1.0 - middle);
	ALBEDO = mix(albedo, puddle_color * 0.8, water * 0.55);
	ROUGHNESS = mix(mix(0.92, 0.5, wetness), 0.12, water);
	SPECULAR = mix(mix(0.15, 0.35, wetness), 0.5, water);
}
