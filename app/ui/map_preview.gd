class_name MapPreview
extends SubViewport
## Draws a course's map picture as the gallery's cards and the course page show it (#192,
## #203): the land around the whole route in black and greys, with its waters, roads and the
## names of its towns, under the path in the logo blue. Captured once when a course is
## prepared and kept in its file, so the gallery needs no map data.

## The picture width the path's lines and markers are measured for; wider pictures scale them.
const BASE_WIDTH: float = 536.0
## Font size (at `BASE_WIDTH`) and colour of the names, by the kind of place.
const LABEL_STYLES: Dictionary[String, Array] = {
	"city": [21.0, Color(0.82, 0.82, 0.82)],
	"town": [19.0, Color(0.7, 0.7, 0.7)],
	"village": [17.0, Color(0.6, 0.6, 0.6)],
}
## Names on one picture at most: enough to place the course, few enough to read on a card.
const MAX_LABELS: int = 6
## Radius of the start and finish markers and of the places' dots, at `BASE_WIDTH`.
const MARKER_RADIUS: float = 5.0
const DOT_RADIUS: float = 2.5

var _canvas: Control = Control.new()
var _mesh: ArrayMesh
var _map: Dictionary = {}
var _track: PackedVector2Array = PackedVector2Array()
## The start and finish markers, in picture pixels.
var _markers: Array[Vector2] = []


func _init() -> void:
	render_target_update_mode = SubViewport.UPDATE_ALWAYS
	# Smooth edges for the land and water, which are drawn as triangles.
	msaa_2d = Viewport.MSAA_4X
	_canvas.draw.connect(_draw_map)
	add_child(_canvas)


## Draws `map` (as `TorqaApp.course_map()` gives it) under `track` and returns the picture
## as PNG; empty when there is nothing to draw, or no renderer to draw with.
static func capture(tree: SceneTree, map: Dictionary, track: PackedVector2Array) -> PackedByteArray:
	if map.is_empty() or track.size() < 2:
		return PackedByteArray()
	var preview: MapPreview = MapPreview.new()
	preview._take(map, track)
	tree.root.add_child(preview)
	await RenderingServer.frame_post_draw
	await RenderingServer.frame_post_draw
	var image: Image = preview.get_texture().get_image()
	preview.queue_free()
	if image == null or image.is_empty():
		return PackedByteArray()
	return image.save_png_to_buffer()


func _take(map: Dictionary, track: PackedVector2Array) -> void:
	_map = map
	_track = track
	size = map["size"]
	_canvas.size = Vector2(size)
	var vertices: PackedVector2Array = map["vertices"]
	if not vertices.is_empty():
		var arrays: Array = []
		arrays.resize(Mesh.ARRAY_MAX)
		arrays[Mesh.ARRAY_VERTEX] = vertices
		arrays[Mesh.ARRAY_COLOR] = map["colors"]
		_mesh = ArrayMesh.new()
		_mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)


func _draw_map() -> void:
	var picture: Vector2 = Vector2(size)
	var unit: float = picture.x / BASE_WIDTH
	var background: Color = _map["background"]
	_canvas.draw_rect(Rect2(Vector2.ZERO, picture), background)
	var frame: Rect2 = _map["frame"]
	var fit: float = picture.x / frame.size.x
	# North up: screen y grows downwards.
	var view: Transform2D = (
		Transform2D()
		. translated(-frame.get_center())
		. scaled(Vector2(fit, -fit))
		. translated(picture / 2.0)
	)
	if _mesh != null:
		_canvas.draw_mesh(_mesh, null, view)
	for line: Dictionary in _map["lines"]:
		var line_points: PackedVector2Array = line["points"]
		var width: float = line["width"]
		var line_color: Color = line["color"]
		_canvas.draw_polyline(view * line_points, line_color, width, true)
	var points: PackedVector2Array = view * _track
	var labels: Array[Dictionary] = _place_labels(view, points, unit)
	# The places' dots under the route, which often runs through them.
	for label: Dictionary in labels:
		var dot: Vector2 = label["dot"]
		var color: Color = label["color"]
		_canvas.draw_circle(dot, DOT_RADIUS * unit, color)
	_canvas.draw_polyline(points, Color(PathCard.BLUE, 0.25), 7.0 * unit, true)
	_canvas.draw_polyline(points, PathCard.BLUE, 2.5 * unit, true)
	_draw_labels(labels, unit)
	_canvas.draw_circle(points[points.size() - 1], MARKER_RADIUS * unit, PathCard.FINISH)
	_canvas.draw_circle(points[0], MARKER_RADIUS * unit, PathCard.START)


## Where the names go, most important first: each beside its place, whole within the picture
## and clear of the route, its markers and the names placed before; over the route only where
## it leaves no side free, as around a town it loops through.
func _place_labels(view: Transform2D, route: PackedVector2Array, unit: float) -> Array[Dictionary]:
	var font: Font = _canvas.get_theme_default_font()
	var bounds: Rect2 = Rect2(Vector2.ZERO, Vector2(size)).grow(-4.0 * unit)
	_markers = [route[0], route[route.size() - 1]]
	var taken: Array[Rect2] = []
	var placed: Array[Dictionary] = []
	for label: Dictionary in _map["labels"]:
		if placed.size() >= MAX_LABELS:
			break
		var kind: String = label["kind"]
		var style: Array = LABEL_STYLES[kind]
		var points: float = style[0]
		var font_size: int = roundi(points * unit)
		var text: String = _shown_name(label, font)
		var text_size: Vector2 = font.get_string_size(
			text, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size
		)
		var place: Vector2 = label["position"]
		var dot: Vector2 = view * place
		var gap: float = (DOT_RADIUS + 2.5) * unit
		# Where the course starts or ends, the marker stands for the place's dot.
		for marker: Vector2 in _markers:
			if dot.distance_to(marker) < 2.0 * MARKER_RADIUS * unit:
				dot = marker
				gap = (MARKER_RADIUS + 3.5) * unit
		var half: Vector2 = text_size / 2.0
		# Right of the place, as maps put names, else left, above or below it.
		var sides: Array[Vector2] = [
			Vector2(gap + half.x, 0.0),
			Vector2(-gap - half.x, 0.0),
			Vector2(0.0, -gap - half.y),
			Vector2(0.0, gap + half.y),
		]
		var boxes: Array[Rect2] = []
		for side: Vector2 in sides:
			boxes.append(Rect2(dot + side - half, text_size))
		var box: Rect2 = _first_clear(boxes, bounds, taken, route, unit)
		if not box.has_area():
			box = _first_clear(boxes, bounds, taken, PackedVector2Array(), unit)
		if text.is_empty() or not box.has_area():
			continue
		taken.append(box)
		taken.append(Rect2(dot, Vector2.ZERO).grow(DOT_RADIUS * unit))
		var ascent: float = font.get_ascent(font_size)
		var entry: Dictionary = {
			"text": text,
			"baseline": box.position + Vector2(0.0, ascent),
			"font_size": font_size,
			"color": style[1],
			"dot": dot,
		}
		placed.append(entry)
	return placed


## The first of `boxes` within `bounds` and clear, as `_clear` has it; an empty one if none is.
func _first_clear(
	boxes: Array[Rect2],
	bounds: Rect2,
	taken: Array[Rect2],
	route: PackedVector2Array,
	unit: float,
) -> Rect2:
	for box: Rect2 in boxes:
		if bounds.encloses(box) and _clear(box, taken, route, unit):
			return box
	return Rect2()


## Whether a name in `box` keeps off the route and its markers and clear of what is `taken`.
func _clear(box: Rect2, taken: Array[Rect2], route: PackedVector2Array, unit: float) -> bool:
	# Its glow may reach under the name's edge, so names fit beside a place it runs through.
	var line: Rect2 = box.grow(3.0 * unit)
	for point: Vector2 in route:
		if line.has_point(point):
			return false
	for marker: Vector2 in _markers:
		if line.intersects(Rect2(marker, Vector2.ZERO).grow(MARKER_RADIUS * unit)):
			return false
	var around: Rect2 = box.grow(4.0 * unit)
	return not taken.any(func(other: Rect2) -> bool: return other.intersects(around))


## The local name where the font has its letters, else the Latin one: Japanese names, say,
## would be drawn as boxes.
func _shown_name(label: Dictionary, font: Font) -> String:
	var local: String = label["name"]
	for i: int in local.length():
		if not font.has_char(local.unicode_at(i)):
			var latin: String = label["latin"]
			return latin
	return local


func _draw_labels(labels: Array[Dictionary], unit: float) -> void:
	var font: Font = _canvas.get_theme_default_font()
	var background: Color = _map["background"]
	var halo: Color = Color(background, 0.85)
	var outline: int = roundi(3.0 * unit)
	for label: Dictionary in labels:
		var text: String = label["text"]
		var baseline: Vector2 = label["baseline"]
		var font_size: int = label["font_size"]
		var color: Color = label["color"]
		_canvas.draw_string_outline(
			font, baseline, text, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size, outline, halo
		)
		_canvas.draw_string(font, baseline, text, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size, color)
