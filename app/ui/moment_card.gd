class_name MomentCard
extends PanelContainer
## A moment of the ride (R124) on a card at the top centre over the HUD: its figure large, its
## line below. It slides in, for an achievement with a short burst of particles in the brand
## blue and the zone colours, stays a few seconds and slides out; moments that come meanwhile
## wait their turn, so two never cover each other, heads-ups first. Group connections (R249)
## come the same way, in red when one dropped and green when it is back, with the burst in
## that colour.

## A card was shown with `text`, e.g. for the overlay to show it in its bar.
signal shown(text: String)

const SHOW_SECONDS: float = 4.0
const SLIDE_SECONDS: float = 0.35
const TOP: float = 80.0
const WIDTH: float = 460.0
## Room kept beside the card in a narrow window.
const SIDE: float = 24.0
const PADDING: float = 16.0

## What is still to show: `[figure, line, accent, heads_up, live]`.
var _queue: Array[Array] = []
## The card on screen's figure and line afresh each frame, if it counts down to something;
## nothing once that is there.
var _live: Callable = Callable()
## Where the card on screen slides out to.
var _above: float = 0.0
var _figure: Label = UiTheme.value(36)
var _line: Label = Label.new()
var _tween: Tween
var _burst: CPUParticles2D = CPUParticles2D.new()
var _style: StyleBoxFlat = UiTheme.panel(0.88)
## The burst's colours for the brand's accent: it and the zone colours.
var _celebration: Gradient = Gradient.new()


func _init() -> void:
	hide()
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	_style.set_border_width_all(0)
	_style.border_width_bottom = 3
	_style.set_content_margin_all(PADDING)
	add_theme_stylebox_override("panel", _style)
	var box: VBoxContainer = VBoxContainer.new()
	box.add_theme_constant_override("separation", 4)
	box.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_figure.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	box.add_child(_figure)
	_line.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_line.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	# A long line in another language stays a short card.
	_line.max_lines_visible = 3
	_line.add_theme_font_size_override("font_size", 16)
	box.add_child(_line)
	add_child(box)
	_build_burst()


## Shows `line` under `figure` (none if empty), in `accent`, after the cards before it. A
## `heads_up` of what is coming comes without particles, as nothing is achieved yet, and before
## the achievements still waiting: late, it would no longer be true. While it shows, `live`
## (if valid) gives its `[figure, line]` afresh each frame, so a distance counts down; an empty
## array ends the card early, as what it told of has come.
func tell(
	figure: String,
	line: String,
	accent: Color = UiTheme.ACCENT,
	heads_up := false,
	live := Callable()
) -> void:
	if line.is_empty():
		return
	var card: Array = [figure, line, accent, heads_up, live]
	if heads_up:
		var first: int = 0
		for waiting: Array in _queue:
			var waiting_heads_up: bool = waiting[3]
			if not waiting_heads_up:
				break
			first += 1
		_queue.insert(first, card)
	else:
		_queue.append(card)
	if not is_showing():
		_next()


## Whether a card is on screen or on its way.
func is_showing() -> bool:
	return _tween != null and _tween.is_running()


## The card on screen: `[figure, line]`, empty when none.
func current() -> PackedStringArray:
	if not is_showing():
		return PackedStringArray()
	return PackedStringArray([_figure.text, _line.text])


## Takes every card away at once, e.g. when the ride ends.
func clear() -> void:
	_queue.clear()
	_live = Callable()
	if _tween != null:
		_tween.kill()
		_tween = null
	_burst.emitting = false
	hide()


func _process(_delta: float) -> void:
	if not _live.is_valid() or not is_showing():
		return
	var now: PackedStringArray = _live.call()
	if now.size() < 2:
		_live = Callable()
		_leave()
		return
	_figure.text = now[0]
	_line.text = now[1]


func _next() -> void:
	_live = Callable()
	if _queue.is_empty():
		hide()
		return
	var card: Array = _queue.pop_front()
	_live = card[4]
	var figure: String = card[0]
	var line: String = card[1]
	var accent: Color = card[2]
	_figure.text = figure
	_figure.visible = not figure.is_empty()
	_figure.add_theme_color_override("font_color", accent)
	_style.border_color = Color(accent, 0.7)
	if accent == UiTheme.ACCENT:
		_burst.color_initial_ramp = _celebration
	else:
		_burst.color_initial_ramp = Gradient.new()
		_ramp(_burst.color_initial_ramp, [accent, accent.lightened(0.3), accent.darkened(0.2)])
	_line.text = line
	# A wrapping label is measured at the width it has: before its first layout that is none,
	# and a card measured then wraps every word and runs off the screen.
	var width: float = minf(WIDTH, get_parent_area_size().x - 2.0 * SIDE)
	custom_minimum_size.x = width
	_line.custom_minimum_size.x = width - 2.0 * PADDING
	reset_size()
	_above = -size.y - 8.0
	position = Vector2((get_parent_area_size().x - size.x) / 2.0, _above)
	modulate.a = 0.0
	show()
	shown.emit(line if figure.is_empty() else "%s  ·  %s" % [figure, line])
	_burst.position = Vector2(size.x / 2.0, size.y)
	_burst.emission_rect_extents = Vector2(size.x / 2.0, 2.0)
	var heads_up: bool = card[3]
	if not heads_up:
		_burst.restart()
	_tween = create_tween()
	_tween.set_ease(Tween.EASE_OUT).set_trans(Tween.TRANS_CUBIC)
	_tween.tween_property(self, "position:y", TOP, SLIDE_SECONDS)
	_tween.parallel().tween_property(self, "modulate:a", 1.0, SLIDE_SECONDS)
	_tween.tween_interval(SHOW_SECONDS)
	_tween.set_ease(Tween.EASE_IN)
	_tween.tween_property(self, "position:y", _above, SLIDE_SECONDS)
	_tween.parallel().tween_property(self, "modulate:a", 0.0, SLIDE_SECONDS)
	_tween.finished.connect(_next)


## Slides the card on screen out now rather than when its time is up.
func _leave() -> void:
	_tween.kill()
	_tween = create_tween()
	_tween.set_ease(Tween.EASE_IN).set_trans(Tween.TRANS_CUBIC)
	_tween.tween_property(self, "position:y", _above, SLIDE_SECONDS)
	_tween.parallel().tween_property(self, "modulate:a", 0.0, SLIDE_SECONDS)
	_tween.finished.connect(_next)


## Not a container child: the panel lays out Controls only, and the particles fly beyond it.
func _build_burst() -> void:
	_burst.emitting = false
	_burst.one_shot = true
	_burst.amount = 70
	_burst.lifetime = 1.1
	_burst.explosiveness = 0.9
	_burst.local_coords = true
	_burst.emission_shape = CPUParticles2D.EMISSION_SHAPE_RECTANGLE
	_burst.direction = Vector2(0.0, 1.0)
	_burst.spread = 60.0
	_burst.initial_velocity_min = 90.0
	_burst.initial_velocity_max = 260.0
	_burst.gravity = Vector2(0.0, 160.0)
	_burst.scale_amount_min = 4.0
	_burst.scale_amount_max = 7.0
	var palette: Array[Color] = [UiTheme.ACCENT]
	for zone: Array in UiTheme.POWER_ZONES.slice(1):
		var color: Color = zone[1]
		palette.append(color)
	_ramp(_celebration, palette)
	var fade: Gradient = Gradient.new()
	fade.set_color(0, Color.WHITE)
	fade.set_color(1, Color(1, 1, 1, 0))
	_burst.color_ramp = fade
	add_child(_burst)


# Each particle takes one colour at random along the ramp.
func _ramp(colors: Gradient, palette: Array[Color]) -> void:
	colors.offsets = PackedFloat32Array()
	colors.colors = PackedColorArray()
	for i: int in range(palette.size()):
		colors.add_point(float(i) / (palette.size() - 1), palette[i])
	colors.interpolation_mode = Gradient.GRADIENT_INTERPOLATE_CONSTANT
