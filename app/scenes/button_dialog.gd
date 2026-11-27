class_name ButtonDialog
extends AcceptDialog
## What a shifter's buttons do (#139, R75, R77): a row per button (a Di2's four D-Fly
## channels, a Zwift Click's + and −, a Click v2's ten, the phone remote's ten), a column per
## kind of press the shifter tells apart, each cell an action. Opened from the pen beside the
## shifter's or the phone remote's row under Devices, so the tab stays one screen whatever
## the shifter (#271).

## The kinds of press, in the core's order.
# i18n-begin
const PRESSES: Array[String] = ["Press", "Hold", "Double press"]
## A Zwift Click's buttons (R75), in the core's numbering (1 and 2).
const CLICK_BUTTONS: Array[String] = ["Plus (+)", "Minus (−)"]
## A Zwift Click v2's buttons (R77), in the core's numbering: + and − first, as a Click's, then
## the right pod's letters and the left pod's arrows.
const CLICK_V2_BUTTONS: Array[String] = [
	"Plus (+)",
	"Minus (−)",
	"Y (right pod)",
	"Z (right pod)",
	"A (right pod)",
	"B (right pod)",
	"Up (left pod)",
	"Down (left pod)",
	"Left (left pod)",
	"Right (left pod)",
]
## The phone remote's buttons (#228), in the core's numbering: the two big ones, then the
## grid below them row by row.
const PHONE_BUTTONS: Array[String] = [
	"Big button 1",
	"Big button 2",
	"Row 1, left",
	"Row 1, right",
	"Row 2, left",
	"Row 2, right",
	"Row 3, left",
	"Row 3, right",
	"Row 4, left",
	"Row 4, right",
]
## What a shifter's buttons can be given (#139), by the core's name for each action.
const ACTION_LABELS: Dictionary[String, String] = {
	"shift_up": "Shift up",
	"shift_down": "Shift down",
	"shift_up_two": "Shift up two gears",
	"shift_down_two": "Shift down two gears",
	"next_camera": "Next camera",
	"camera_left": "Camera round to the left",
	"camera_right": "Camera round to the right",
	"camera_nearer": "Camera nearer",
	"camera_further": "Camera further away",
	"overlay": "Overlay on / off",
	"play_pause": "Music: play / pause",
	"next_track": "Music: next track",
	"previous_track": "Music: previous track",
	"pause": "Pause / resume",
	"difficulty_up": "Trainer difficulty +10 %",
	"difficulty_down": "Trainer difficulty −10 %",
	"intensity_up": "Workout intensity +5 %",
	"intensity_down": "Workout intensity −5 %",
	"skip_step": "Workout: skip step",
	"finish_ride": "Finish & save",
}
# i18n-end
## A Di2's buttons are its four D-Fly channels.
const CHANNELS: int = 4
## The most buttons a shifter has, a Click v2's ten: the rows built, shown as far as needed.
const BUTTON_ROWS: int = 10
## A Zwift Click, original or v2, tells presses and holds apart, not double presses.
const CLICK_PRESSES: int = 2
## The core's names for the shifters.
const DI2: String = "di2"
const ZWIFT_CLICK: String = "zwift_click"
const ZWIFT_CLICK_V2: String = "zwift_click_v2"
const PHONE_REMOTE: String = "phone_remote"

var _orqwa: OrqwaApp
var _grid: GridContainer = GridContainer.new()
var _press_headings: Array[Label] = []
var _channel_captions: Array[Label] = []
var _assignments: Array[OptionButton] = []
## The shifter shown, by the core's name.
var _model: String = DI2


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa


## Opens the dialog for the shifter `model` (the core's name) called `device_name`, with the
## assignments as the core has them.
func open(model: String, device_name: String) -> void:
	_model = model
	# A shifter by its own name; the remote is ORQWA's.
	title = (
		tr("What the remote's buttons do")
		if model == PHONE_REMOTE
		else tr("What the buttons of %s do") % device_name
	)
	_layout()
	if _orqwa != null:
		var assigned: Array = _orqwa.button_map(_model)
		for channel: int in range(assigned.size()):
			var presses: PackedStringArray = assigned[channel]
			for press: int in range(presses.size()):
				var option: OptionButton = _assignments[channel * PRESSES.size() + press]
				for i: int in range(option.item_count):
					if option.get_item_metadata(i) == presses[press]:
						option.select(i)
	# As tall as a Click v2's ten rows where the window has room; they scroll in a small one.
	UiScale.popup(self, Vector2i(980, 780))


func _init() -> void:
	ok_button_text = tr("Done")
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	var note: Label = Label.new()
	note.text = tr(
		"Each press shifts, changes the view, controls music or the workout, or does nothing."
	)
	note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	note.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(note)
	var scroll: ScrollContainer = ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	UiTheme.scroll_down_only(scroll)
	_grid.columns = 1 + PRESSES.size()
	_grid.add_theme_constant_override("h_separation", 16)
	_grid.add_theme_constant_override("v_separation", 10)
	var corner: Control = Control.new()
	corner.custom_minimum_size = Vector2(RideOptions.CAPTION_WIDTH, 0)
	_grid.add_child(corner)
	for heading: String in PRESSES:
		var label: Label = Label.new()
		label.text = heading
		_press_headings.append(label)
		_grid.add_child(label)
	for channel: int in range(1, BUTTON_ROWS + 1):
		var caption: Label = Label.new()
		caption.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		_channel_captions.append(caption)
		_grid.add_child(caption)
		for press: int in range(PRESSES.size()):
			var option: OptionButton = OptionButton.new()
			option.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
			option.custom_minimum_size = Vector2(240, 0)
			option.add_item("")
			option.set_item_metadata(0, "")
			for action: String in OrqwaApp.button_actions():
				option.add_item("")
				option.set_item_metadata(option.item_count - 1, action)
			option.item_selected.connect(
				func(index: int) -> void:
					var action: String = option.get_item_metadata(index)
					_orqwa.assign_button(_model, channel, press, action)
			)
			_assignments.append(option)
			_grid.add_child(option)
	scroll.add_child(_grid)
	rows.add_child(scroll)
	add_child(rows)
	_label_actions()
	_layout()


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED and is_node_ready():
		ok_button_text = tr("Done")
		_label_actions()
		_layout()


## Shows the rows and columns of the shifter's buttons and presses, captioned for it.
func _layout() -> void:
	var captions: Array[String] = _captions()
	var buttons: int = captions.size()
	var presses: int = PRESSES.size()
	if _model in [ZWIFT_CLICK, ZWIFT_CLICK_V2]:
		presses = CLICK_PRESSES
	elif _model == PHONE_REMOTE:
		# A press on the page acts when the finger lands: there is no hold to wait for.
		presses = 1
	_grid.columns = 1 + presses
	for press: int in range(PRESSES.size()):
		_press_headings[press].text = tr(PRESSES[press])
		_press_headings[press].visible = press < presses
	for channel: int in range(BUTTON_ROWS):
		var caption: Label = _channel_captions[channel]
		caption.visible = channel < buttons
		if channel < buttons:
			caption.text = captions[channel]
		for press: int in range(PRESSES.size()):
			var option: OptionButton = _assignments[channel * PRESSES.size() + press]
			option.visible = channel < buttons and press < presses
			# The remote's page ends the ride from its header, held: never on one tap.
			for i: int in range(option.item_count):
				var action: String = option.get_item_metadata(i)
				option.set_item_disabled(i, _model == PHONE_REMOTE and action == "finish_ride")


## What the shifter's buttons are called, in the core's numbering.
func _captions() -> Array[String]:
	var captions: Array[String] = []
	var named: Array[String] = []
	if _model == ZWIFT_CLICK_V2:
		named = CLICK_V2_BUTTONS
	elif _model == ZWIFT_CLICK:
		named = CLICK_BUTTONS
	elif _model == PHONE_REMOTE:
		named = PHONE_BUTTONS
	for caption: String in named:
		captions.append(tr(caption))
	if named.is_empty():
		for channel: int in range(1, CHANNELS + 1):
			captions.append(tr("D-Fly channel %d") % channel)
	return captions


## Names the actions in the interface language.
func _label_actions() -> void:
	for option: OptionButton in _assignments:
		for i: int in range(option.item_count):
			var action: String = option.get_item_metadata(i)
			var label: String = ACTION_LABELS.get(action, action)
			option.set_item_text(i, tr(label) if action else tr("Nothing"))
