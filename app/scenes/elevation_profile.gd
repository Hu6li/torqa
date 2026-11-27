class_name ElevationProfile
extends Control
## The route's elevation profile as a clean area chart with a crisp outline and the rider's
## position: ridden part in the accent colour and the rest light, or, if the rider opts in
## (R242), coloured by gradient (R241): green descents, yellow flats and red climbs, darker the
## steeper, with the ridden part solid and the rest translucent.

## A simulated ride's rider should jump to `distance_m` along the route (#53).
signal jump_requested(distance_m: float)
## A drag across the profile marks the stretch from `start_m` to `end_m` (R302), or moves an
## end or the whole of the one marked; sent as the drag goes, so whatever shows its ends follows
## the pointer.
signal stretch_picked(start_m: float, end_m: float)

## What a press on a stretchable profile takes hold of: a new stretch, an end of the one
## marked, or all of it.
enum Grab { NEW, START, END, WHOLE }

## How far the pointer moves before a press counts as a drag rather than a click.
const DRAG_PX: float = 6.0
## How near an end of the marked stretch a press takes that end.
const HANDLE_PX: float = 8.0

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
## The outline takes the gradient colour too, barely brighter, over a faint glow: enough to
## lift the line off the area without making the chart look shiny.
const LINE_LIGHTEN: float = 0.08
const LINE_WIDTH: float = 1.5
const GLOW_WIDTH: float = 4.0
const GLOW_ALPHA: float = 0.08
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
		_press_x = -1.0
		_dragging = false
		mouse_filter = MOUSE_FILTER_STOP if value else MOUSE_FILTER_IGNORE
		tooltip_text = tr("Click: jump there") if value else ""
		mouse_default_cursor_shape = CURSOR_POINTING_HAND if value else CURSOR_ARROW

## With [member jumpable], a drag marks a stretch ([signal stretch_picked]) and a click goes
## out on release, as only then is it known not to be a drag. The stretch stays on the profile
## until this ends, its ends and its middle to be dragged on.
var stretchable: bool = false:
	set(value):
		stretchable = value
		_press_x = -1.0
		_dragging = false
		if not value:
			_draft.clear()
		queue_redraw()

## `(distance m, elevation m)` points.
var _profile: PackedVector2Array = PackedVector2Array()
## Where the left button went down while stretchable, negative when it is up.
var _press_x: float = -1.0
## The press has moved far enough to be a drag.
var _dragging: bool = false
var _grab: Grab = Grab.NEW
## The marked stretch as it was when the button went down.
var _grabbed: Array[float] = []
## The stretch being marked, `[start m, end m]`, empty when none.
var _draft: Array[float] = []
var _rider_distance: float = 0.0
var _ghost_distance: float = -1.0
var _peers: Array[Dictionary] = []
## `[start m, end m, colour]` per climb.
var _climbs: Array[Array] = []
## `[start m, end m]` per segment of the rider's (R302).
var _segments: Array[Array] = []
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


## Marks the rider's segments (`OrqwaApp.segments()`, R302) with a band along the top and their
## ends with faint lines, so they read apart from the climbs at the bottom.
func set_segments(segments: Array) -> void:
	_segments.clear()
	for segment: Dictionary in segments:
		_segments.append([segment["start_m"], segment["end_m"]])
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
	var ends: Color = Color(UiTheme.SEGMENT_COLOR, 0.35)
	for segment: Array in _segments:
		var start_m: float = segment[0]
		var end_m: float = segment[1]
		var from: float = _to_screen(Vector2(start_m, 0.0)).x
		var to: float = _to_screen(Vector2(end_m, 0.0)).x
		draw_rect(Rect2(from, 0.0, to - from, 3.0), UiTheme.SEGMENT_COLOR)
		draw_line(Vector2(from, 0.0), Vector2(from, bottom), ends, 1.0)
		draw_line(Vector2(to, 0.0), Vector2(to, bottom), ends, 1.0)
	if not _draft.is_empty():
		_draw_draft(bottom)

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


## The outline in each point's gradient colour over a faint glow; dimmer ahead.
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


## Shows the stretch being marked, from `start_m` to `end_m` either way (as typed in, say),
## until [member stretchable] ends.
func set_draft(start_m: float, end_m: float) -> void:
	_draft.assign([minf(start_m, end_m), maxf(start_m, end_m)])
	queue_redraw()


## The marked stretch, shaded, with a grip at each end to drag.
func _draw_draft(bottom: float) -> void:
	var from: float = _to_screen(Vector2(_draft[0], 0.0)).x
	var to: float = _to_screen(Vector2(_draft[1], 0.0)).x
	draw_rect(Rect2(from, 0.0, to - from, bottom), Color(UiTheme.SEGMENT_COLOR, 0.18))
	draw_rect(Rect2(from, 0.0, to - from, 3.0), UiTheme.SEGMENT_COLOR)
	for x: float in [from, to]:
		draw_line(Vector2(x, 0.0), Vector2(x, bottom), UiTheme.SEGMENT_COLOR, 2.0)
		draw_rect(Rect2(x - 3.0, bottom / 2.0 - 9.0, 6.0, 18.0), UiTheme.SEGMENT_COLOR)


func _gui_input(event: InputEvent) -> void:
	if not jumpable or _profile.size() < 2:
		return
	var motion: InputEventMouseMotion = event as InputEventMouseMotion
	if motion != null:
		if _press_x < 0.0:
			if stretchable:
				mouse_default_cursor_shape = _cursor(_grab_at(motion.position.x))
			return
		if not _dragging and absf(motion.position.x - _press_x) < DRAG_PX:
			return
		_dragging = true
		_drag_to(motion.position.x)
		accept_event()
		return
	var click: InputEventMouseButton = event as InputEventMouseButton
	if click == null or click.button_index != MOUSE_BUTTON_LEFT:
		return
	if not stretchable:
		if click.pressed:
			jump_requested.emit(_distance_at(click.position.x))
			accept_event()
		return
	if click.pressed:
		_press_x = click.position.x
		_dragging = false
		_grab = _grab_at(_press_x)
		_grabbed = _draft.duplicate()
	elif _press_x >= 0.0:
		if _dragging:
			_drag_to(click.position.x)
		else:
			jump_requested.emit(_distance_at(_press_x))
		_press_x = -1.0
		_dragging = false
	accept_event()


## What a press at `x` takes hold of; on a short stretch the nearer end.
func _grab_at(x: float) -> Grab:
	if _draft.is_empty():
		return Grab.NEW
	var from: float = _to_screen(Vector2(_draft[0], 0.0)).x
	var to: float = _to_screen(Vector2(_draft[1], 0.0)).x
	if absf(x - from) <= HANDLE_PX and absf(x - from) <= absf(x - to):
		return Grab.START
	if absf(x - to) <= HANDLE_PX:
		return Grab.END
	if x > from and x < to:
		return Grab.WHOLE
	return Grab.NEW


func _cursor(grab: Grab) -> CursorShape:
	match grab:
		Grab.START, Grab.END:
			return CURSOR_HSIZE
		Grab.WHOLE:
			return CURSOR_MOVE
	return CURSOR_POINTING_HAND


## The drag from where the button went down to `x`: a new stretch either way, an end of the
## marked one moved (past the other, they swap), or all of it moved within the course.
func _drag_to(x: float) -> void:
	var at: float = _distance_at(x)
	var start: float = _distance_at(_press_x)
	var end: float = at
	match _grab:
		Grab.START:
			start = at
			end = _grabbed[1]
		Grab.END:
			start = _grabbed[0]
		Grab.WHOLE:
			var length: float = _profile[_profile.size() - 1].x
			var shift: float = clampf(at - start, -_grabbed[0], length - _grabbed[1])
			start = _grabbed[0] + shift
			end = _grabbed[1] + shift
	set_draft(start, end)
	stretch_picked.emit(_draft[0], _draft[1])


func _distance_at(x: float) -> float:
	return clampf(x / maxf(size.x, 1.0), 0.0, 1.0) * _profile[_profile.size() - 1].x


func _to_screen(point: Vector2) -> Vector2:
	var length: float = maxf(_profile[_profile.size() - 1].x, 1.0)
	var height: float = size.y - PADDING * 2.0
	var x: float = point.x / length * size.x
	var y: float = (
		PADDING + (1.0 - (point.y - _min_elevation) / (_max_elevation - _min_elevation)) * height
	)
	return Vector2(x, y)
