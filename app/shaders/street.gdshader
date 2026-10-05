// Streets, tracks and paths of the map around the route (R45, ADR 0011): flat palette colours —
// blue-grey asphalt for paved streets, sandy gravel for tracks and paths, with a strip of grass
// down the middle of tracks — faceted like the ground. UV.x runs 0–1 across, UV.y is the
// distance along in metres.
shader_type spatial;

uniform vec3 surface_color : source_color = vec3(0.557, 0.584, 0.608);
uniform vec3 grass_color : source_color = vec3(0.698, 0.757, 0.471);
// 1 for tracks: grass in the middle where wheels do not run.
uniform float middle_grass : hint_range(0.0, 1.0) = 0.0;
uniform float wetness : hint_range(0.0, 1.0) = 0.0;

void fragment() {
	NORMAL = normalize(cross(dFdy(VERTEX), dFdx(VERTEX)));
	float off_centre = abs(UV.x - 0.5);
	float soft = max(fwidth(off_centre), 1e-4);
	float middle = middle_grass * (1.0 - smoothstep(0.12 - soft, 0.12 + soft, off_centre));
	vec3 surface = surface_color * (1.0 - wetness * 0.25);
	ALBEDO = mix(surface, grass_color, middle);
	ROUGHNESS = mix(0.92, 0.6, wetness);
	SPECULAR = 0.15;
}
