class_name ElevationProfile
extends Control
## The route's elevation profile as a clean area chart with a crisp outline and the rider's
## position: ridden part in the accent colour and the rest light, or, if the rider opts in
## (R242), coloured by gradient (R241): green descents, yellow flats and red climbs, darker the
## steeper, with the ridden part solid and the rest translucent.

## A simulated ride's rider should jump to `distance_m` along the route (#53).
signal jump_requested(distance_m: float)

const PADDING: float = 2.0
## The smallest elevation range drawn, so flat routes do not look mountainous.
const MIN_RANGE_M: float = 40.0
const OUTLINE: Color = Color(1, 1, 1, 0.75)
const AHEAD: Color = Color(1, 1, 1, 0.13)
## The gradient, in percent either way, drawn in the darkest shade (R241); everything in between
## spreads over the scale, so 10% climbs already read clearly red and only walls go deep red.
const STEEPEST_PERCENT: float = 25.0
## Emerald, sunny yellow and rose read as green, yellow and red but sit better on the dark UI
## than pure primaries.
const STEEP_DESCENT: Color = Color("#065f46")
const DESCENT: Color = Color("#10b981")
const FLAT: Color = Color("#fde047")
const CLIMB: Color = Color("#f43f5e")
const STEEP_CLIMB: Color = Color("#9f1239")
const RIDDEN_ALPHA: float = 0.9
## During a ride the profile is small and over the 3D world: the colours stay strong there so
## they read at a glance, with the part still ahead lighter.
const OVERLAY_RIDDEN_ALPHA: float = 0.85
const AHEAD_ALPHA: float = 0.4
## The area fades towards the bottom to this share of its opacity at the curve, so the colour
## sits on the line and the chart stays light; less so in the ride's small panel.
const FILL_FADE: float = 0.12
const OVERLAY_FILL_FADE: float = 0.35
## The outline takes the gradient colour too, a little brighter, over a soft glow.
const LINE_LIGHTEN: float = 0.25
const LINE_WIDTH: float = 2.0
const GLOW_WIDTH: float = 7.0
const GLOW_ALPHA: float = 0.22
const AHEAD_LINE_ALPHA: float = 0.6
## Gradients are averaged over this much road around each point, and over a 300th of a long
## course, so neighbouring points do not flicker between colours on a few metres of noise.
const SMOOTHING_M: float = 50.0
const SMOOTHING_SHARE: float = 1.0 / 300.0

## With gradient colours, during a ride the part still ahead is translucent; a course view shows
## all of it solid. The ride screen's scene turns it on.
@export var shows_progress: bool = false:
	set(value):
		shows_progress = value
		queue_redraw()

## The rider's choice to colour the area by gradient (R242, the profile's `gradient_colours`).
var gradient_colours: bool = false:
	set(value):
		gradient_colours = value
		queue_redraw()

## Clicks jump the rider there (simulated rides).
var jumpable: bool = false:
	set(value):
		jumpable = value
		mouse_filter = MOUSE_FILTER_STOP if value else MOUSE_FILTER_IGNORE
		tooltip_text = tr("Click: jump there") if value else ""
		mouse_default_cursor_shape = CURSOR_POINTING_HAND if value else CURSOR_ARROW

## `(distance m, elevation m)` points.
var _profile: PackedVector2Array = PackedVector2Array()
var _rider_distance: float = 0.0
var _ghost_distance: float = -1.0
var _peers: Array[Dictionary] = []
## `[start m, end m, colour]` per climb.
var _climbs: Array[Array] = []
var _min_elevation: float = 0.0
var _max_elevation: float = 0.0
## The gradient colour at each profile point; the area blends between them.
var _colors: PackedColorArray = PackedColorArray()


func set_profile(profile: PackedVector2Array) -> void:
	_profile = profile
	_rider_distance = 0.0
	_colors.clear()
	if _profile.size() >= 2:
		var length: float = _profile[_profile.size() - 1].x - _profile[0].x
		var window: float = maxf(SMOOTHING_M, length * SMOOTHING_SHARE)
		for grade: float in point_grades(_profile, window):
			_colors.append(grade_color(grade))
	if not _profile.is_empty():
		_min_elevation = _profile[0].y
		_max_elevation = _profile[0].y
		for point: Vector2 in _profile:
			_min_elevation = minf(_min_elevation, point.y)
			_max_elevation = maxf(_max_elevation, point.y)
		var missing: float = MIN_RANGE_M - (_max_elevation - _min_elevation)
		if missing > 0.0:
			_min_elevation -= missing / 2.0
			_max_elevation += missing / 2.0
	queue_redraw()


## Marks the route's climbs (`OrqwaApp.climbs()["climbs"]`) with a band in their category colour.
func set_climbs(climbs: Array) -> void:
	_climbs.clear()
	for climb: Dictionary in climbs:
		var category: String = climb["category"]
		_climbs.append(
			[climb["start_m"], climb["end_m"], UiTheme.CLIMB_COLORS.get(category, Color.GRAY)]
		)
	queue_redraw()


## The ghost's distance along the route, or a negative value to hide it.
func set_ghost_distance(distance_m: float) -> void:
	_ghost_distance = distance_m
	queue_redraw()


func set_rider_distance(distance_m: float) -> void:
	_rider_distance = distance_m
	queue_redraw()


## The current group riders, in the same identity colours as the road and map.
func set_group_riders(peers: Array) -> void:
	_peers.assign(peers)
	queue_redraw()


func _draw() -> void:
	if _profile.size() < 2:
		return
	var bottom: float = size.y
	var outline: PackedVector2Array = PackedVector2Array()
	for i: int in range(_profile.size()):
		outline.append(_to_screen(_profile[i]))
	var ridden_color: Color = Color(UiTheme.ACCENT, 0.55)
	var solid: float = OVERLAY_RIDDEN_ALPHA if shows_progress else RIDDEN_ALPHA
	var fade: float = OVERLAY_FILL_FADE if shows_progress else FILL_FADE
	for i: int in range(_profile.size() - 1):
		var from: Vector2 = _profile[i]
		var to: Vector2 = _profile[i + 1]
		if not gradient_colours:
			var plain: Color = ridden_color if from.x < _rider_distance else AHEAD
			_fill(outline[i], outline[i + 1], bottom, plain, plain)
			continue
		# A segment the rider is on splits there, so the ridden part ends exactly at the rider.
		var ridden: float = _rider_distance if shows_progress else to.x
		var t: float = (clampf(ridden, from.x, to.x) - from.x) / maxf(to.x - from.x, 0.001)
		var at: Vector2 = _to_screen(from.lerp(to, t))
		var middle: Color = _colors[i].lerp(_colors[i + 1], t)
		_fill(outline[i], at, bottom, Color(_colors[i], solid), Color(middle, solid), fade)
		_fill(
			at,
			outline[i + 1],
			bottom,
			Color(middle, AHEAD_ALPHA),
			Color(_colors[i + 1], AHEAD_ALPHA),
			fade
		)
	if gradient_colours:
		_draw_gradient_line(outline)
	else:
		draw_polyline(outline, OUTLINE, 1.5, true)
	for climb: Array in _climbs:
		var start_m: float = climb[0]
		var end_m: float = climb[1]
		var color: Color = climb[2]
		var from: float = _to_screen(Vector2(start_m, 0.0)).x
		var to: float = _to_screen(Vector2(end_m, 0.0)).x
		draw_rect(Rect2(from, bottom - 3.0, to - from, 3.0), color)

	for peer: Dictionary in _peers:
		var id: int = peer["id"]
		var distance: float = peer["distance_m"]
		var position: Vector2 = _to_screen(Vector2(distance, _elevation_at(distance)))
		draw_circle(position, 4.0, UiTheme.group_color(id))
	if _ghost_distance >= 0.0:
		var ghost: Vector2 = _to_screen(Vector2(_ghost_distance, _elevation_at(_ghost_distance)))
		draw_circle(ghost, 4.0, UiTheme.GHOST_COLOR)
	var rider: Vector2 = _to_screen(Vector2(_rider_distance, _elevation_at(_rider_distance)))
	draw_line(Vector2(rider.x, 0), Vector2(rider.x, bottom), Color(UiTheme.ACCENT, 0.9), 1.5)
	draw_circle(rider, 5.0, Color.WHITE)
	draw_circle(rider, 3.0, UiTheme.ACCENT)


## The area colour for a gradient in percent (R241): yellow on the flat, through green to dark
## green going down and through red to dark red going up, darkest from [constant STEEPEST_PERCENT].
static func grade_color(grade_percent: float) -> Color:
	var steepness: float = clampf(absf(grade_percent) / STEEPEST_PERCENT, 0.0, 1.0)
	var near: Color = CLIMB if grade_percent > 0.0 else DESCENT
	var far: Color = STEEP_CLIMB if grade_percent > 0.0 else STEEP_DESCENT
	if steepness < 0.5:
		return FLAT.lerp(near, steepness * 2.0)
	return near.lerp(far, (steepness - 0.5) * 2.0)


## The gradient in percent at each of `profile`'s `(distance m, elevation m)` points, over
## `window_m` of road centred on it (shortened at the ends).
static func point_grades(profile: PackedVector2Array, window_m: float) -> PackedFloat32Array:
	var grades: PackedFloat32Array = PackedFloat32Array()
	var last: int = profile.size() - 1
	var behind: int = 0
	var ahead: int = 0
	for point: Vector2 in profile:
		var from: float = maxf(point.x - window_m / 2.0, profile[0].x)
		var to: float = minf(point.x + window_m / 2.0, profile[last].x)
		while behind < last - 1 and profile[behind + 1].x < from:
			behind += 1
		while ahead < last - 1 and profile[ahead + 1].x < to:
			ahead += 1
		var rise: float = _height(profile, ahead, to) - _height(profile, behind, from)
		grades.append(rise / maxf(to - from, 0.001) * 100.0)
	return grades


## The elevation at `distance_m` on the segment from point `index` to the next.
static func _height(profile: PackedVector2Array, index: int, distance_m: float) -> float:
	var a: Vector2 = profile[index]
	var b: Vector2 = profile[index + 1]
	return lerpf(a.y, b.y, clampf((distance_m - a.x) / maxf(b.x - a.x, 0.001), 0.0, 1.0))


## Fills the area under `a`–`b`, blending from colour `left` to `right` and fading to `fade` of
## their opacity at the bottom.
func _fill(
	a: Vector2, b: Vector2, bottom: float, left: Color, right: Color, fade: float = 1.0
) -> void:
	if b.x - a.x < 0.01:
		return
	var left_bottom: Color = Color(left, left.a * fade)
	var right_bottom: Color = Color(right, right.a * fade)
	# Drawn as quads without triangulation, which fails on flat segments at the bottom.
	draw_primitive(
		PackedVector2Array([a, b, Vector2(b.x, bottom), Vector2(a.x, bottom)]),
		PackedColorArray([left, right, right_bottom, left_bottom]),
		PackedVector2Array()
	)


## The outline in each point's gradient colour, brighter, over a soft glow; dimmer ahead.
func _draw_gradient_line(outline: PackedVector2Array) -> void:
	var line: PackedColorArray = PackedColorArray()
	var glow: PackedColorArray = PackedColorArray()
	for i: int in range(_profile.size()):
		var ahead: bool = shows_progress and _profile[i].x > _rider_distance
		var color: Color = _colors[i].lightened(LINE_LIGHTEN)
		var alpha: float = AHEAD_LINE_ALPHA if ahead else 1.0
		line.append(Color(color, alpha))
		glow.append(Color(color, GLOW_ALPHA * alpha))
	draw_polyline_colors(outline, glow, GLOW_WIDTH, true)
	draw_polyline_colors(outline, line, LINE_WIDTH, true)


func _elevation_at(distance_m: float) -> float:
	for i: int in range(_profile.size() - 1):
		var a: Vector2 = _profile[i]
		var b: Vector2 = _profile[i + 1]
		if distance_m <= b.x:
			var t: float = (distance_m - a.x) / maxf(b.x - a.x, 0.001)
			return lerpf(a.y, b.y, clampf(t, 0.0, 1.0))
	return _profile[_profile.size() - 1].y


func _gui_input(event: InputEvent) -> void:
	var click: InputEventMouseButton = event as InputEventMouseButton
	if not jumpable or _profile.size() < 2 or click == null or not click.pressed:
		return
	if click.button_index == MOUSE_BUTTON_LEFT:
		var length: float = _profile[_profile.size() - 1].x
		jump_requested.emit(clampf(click.position.x / maxf(size.x, 1.0), 0.0, 1.0) * length)
		accept_event()


func _to_screen(point: Vector2) -> Vector2:
	var length: float = maxf(_profile[_profile.size() - 1].x, 1.0)
	var height: float = size.y - PADDING * 2.0
	var x: float = point.x / length * size.x
	var y: float = (
		PADDING + (1.0 - (point.y - _min_elevation) / (_max_elevation - _min_elevation)) * height
	)
	return Vector2(x, y)
