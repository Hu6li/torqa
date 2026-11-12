class_name ProfileForm
extends GridContainer
## A rider's figures and choices, the fields as `OrqwaApp.profile()` has them: weights, FTP,
## maximum heart rate, trainer difficulty, units, language, the rider on the bike, the ride map,
## the voice the ride's moments are told in (R124) and the drivetrain (R9). The rider settings
## and the ride settings' Profile tab (R98) both show it, so a rider is set up the same way
## before and during a ride.

## The rider changed a field; read them with `profile()`.
signal changed

const UNITS: Array[String] = ["metric", "imperial"]
const DRIVETRAINS: Array[String] = ["cassette", "single_cog"]
## How the ride's minimap is drawn (R70).
const MAP_STYLES: Array[String] = ["coloured", "dark"]
## How the ride's moments are told (R124).
const VOICES: Array[String] = ["casual", "plain", "off"]
const CALORIE_SEXES: Array[String] = ["", "female", "male"]
const CALORIE_EFFORTS: Array[String] = ["", "easy", "moderate", "hard"]
## Interface languages: locale code and name in that language; "" follows the system.
const LANGUAGES: Array[Array] = [["", "System language"], ["en", "English"], ["de", "Deutsch"]]

var _rider_mass: SpinBox = _spin(30.0, 200.0, 0.5, " kg")
var _bike_mass: SpinBox = _spin(3.0, 40.0, 0.1, " kg")
var _ftp: SpinBox = _spin(50.0, 600.0, 1.0, " W")
var _max_heart_rate: SpinBox = _spin(100.0, 230.0, 1.0, " bpm")
var _difficulty: SpinBox = _spin(0.0, 100.0, 5.0, " %")
## Rides a week the rider plans (R124), 0 for none.
var _rides_per_week: SpinBox = _spin(0.0, 14.0, 1.0, "")
var _units: OptionButton = OptionButton.new()
var _language: OptionButton = OptionButton.new()
var _avatar: OptionButton = OptionButton.new()
var _map_style: OptionButton = OptionButton.new()
## Elevation profile plain or coloured by gradient (R242), plain unless chosen; a dropdown like
## the ride map's, so the rider settings stay alike.
var _gradient_colours: OptionButton = OptionButton.new()
var _voice: OptionButton = OptionButton.new()
## A cassette, or a single cog with virtual gears (R9): its chainring and cog then.
var _drivetrain: OptionButton = OptionButton.new()
var _chainring: SpinBox = _spin(20.0, 60.0, 1.0, " T")
var _cog: SpinBox = _spin(9.0, 36.0, 1.0, " T")
var _teeth_rows: Array[Control] = []
var _difficulty_row: Array[Control] = []
var _calorie_age: SpinBox = _spin(0.0, 100.0, 1.0, "")
## Input metadata; the core infers a birth year without rebasing an unchanged open form.
var _calorie_age_year: int = 0
var _calorie_sex: OptionButton = OptionButton.new()
var _calorie_effort: OptionButton = OptionButton.new()


func _init() -> void:
	columns = 2
	add_theme_constant_override("h_separation", 24)
	add_theme_constant_override("v_separation", 12)
	_units.add_item(tr("Metric (km, kg)"))
	_units.add_item(tr("Imperial (mi, lb)"))
	# In the order of RiderAvatar.RIDERS.
	_avatar.add_item(tr("Female rider"))
	_avatar.add_item(tr("Male rider"))
	# In the order of MAP_STYLES.
	_map_style.add_item(tr("Colored"))
	_map_style.add_item(tr("Dark, like the course cards"))
	# Plain first: index 1 is the gradient colours.
	_gradient_colours.add_item(tr("Plain"))
	_gradient_colours.add_item(tr("By gradient: green down, yellow flat, red up"))
	# In the order of VOICES.
	_voice.add_item(tr("Casual and cheering"))
	_voice.add_item(tr("Plain facts"))
	_voice.add_item(tr("Off"))
	_voice.tooltip_text = tr(
		"How ORQWA tells you of climbs topped, milestones and records during the ride"
	)
	_rides_per_week.tooltip_text = tr(
		"The ride's summary counts your rides this week against it; 0 if you plan no number"
	)
	# In the order of DRIVETRAINS.
	_drivetrain.add_item(tr("Cassette: shift on the bike"))
	_drivetrain.add_item(tr("Single cog: virtual gears"))
	_drivetrain.tooltip_text = tr(
		"On a single cog such as the Zwift Cog, ORQWA shifts 24 virtual gears: ↑ and ↓ while riding"
	)
	_drivetrain.item_selected.connect(func(_index: int) -> void: _show_teeth())
	_calorie_age.tooltip_text = tr(
		"Optional age for heart-rate calories (18–100). Increases each New Year. 0 means not set."
	)
	_calorie_sex.add_item(tr("Not set"))
	_calorie_sex.add_item(tr("Female"))
	_calorie_sex.add_item(tr("Male"))
	_calorie_sex.tooltip_text = tr(
		"Optional sex for heart-rate calories. Separate from your avatar. Power needs no sex."
	)
	_calorie_effort.add_item(tr("No estimate"))
	_calorie_effort.add_item(tr("Easy effort"))
	_calorie_effort.add_item(tr("Moderate effort"))
	_calorie_effort.add_item(tr("Hard effort"))
	_calorie_effort.tooltip_text = tr(
		"Rough estimate without power or usable heart rate. Counts time even without pedalling."
	)
	# Language names stay in their own language; only "System language" is translated.
	_language.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	for language: Array in LANGUAGES:
		var code: String = language[0]
		var language_name: String = language[1]
		_language.add_item(tr(language_name) if code.is_empty() else language_name)
	# i18n-begin
	for row: Array in [
		["Weight", _rider_mass],
		["Bike weight", _bike_mass],
		["FTP", _ftp],
		["Max heart rate", _max_heart_rate],
		["Trainer difficulty", _difficulty],
		["Units", _units],
		["Language", _language],
		["Rider on the bike", _avatar],
		["Ride map", _map_style],
		["Elevation colors", _gradient_colours],
		["Voice", _voice],
		["Rides planned per week", _rides_per_week],
		["Drivetrain", _drivetrain],
		["Chainring", _chainring],
		["Cog", _cog],
		["Age", _calorie_age],
		["Sex", _calorie_sex],
		["Calories", _calorie_effort],
	]:
		# i18n-end
		var caption: Label = Label.new()
		caption.text = row[0]
		var field: Control = row[1]
		if field in [_calorie_age, _calorie_sex, _calorie_effort]:
			caption.tooltip_text = field.tooltip_text
			caption.mouse_filter = Control.MOUSE_FILTER_PASS
		# The fields grow with the dialog rather than staying fixed in the middle (R53).
		field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		add_child(caption)
		add_child(field)
		if field in [_chainring, _cog]:
			_teeth_rows.append_array([caption, field])
		elif field == _difficulty:
			_difficulty_row.append_array([caption, field])
		if field == _calorie_age:
			_calorie_age.value_changed.connect(_on_calorie_age_changed)
		elif field is SpinBox:
			(field as SpinBox).value_changed.connect(func(_value: float) -> void: changed.emit())
		else:
			(field as OptionButton).item_selected.connect(func(_index: int) -> void: changed.emit())


## Shows `profile` (shaped like `OrqwaApp.profile()`; a new rider's if it has no fields)
## without emitting `changed`.
func edit(profile: Dictionary) -> void:
	_calorie_age_year = profile.get("calorie_age_year", OrqwaApp.calendar_year())
	for field: Array in [
		[_rider_mass, "rider_mass_kg", 75.0],
		[_bike_mass, "bike_mass_kg", 8.0],
		[_ftp, "ftp_w", 200.0],
		[_max_heart_rate, "max_heart_rate_bpm", 185.0],
		[_difficulty, "default_difficulty_pct", 50.0],
		[_rides_per_week, "rides_per_week", 0.0],
		[_chainring, "chainring", 50.0],
		[_cog, "cog", 14.0],
		[_calorie_age, "calorie_age", 0.0],
	]:
		var spin: SpinBox = field[0]
		var value: float = profile.get(field[1], field[2])
		if spin == _calorie_age:
			spin.max_value = maxf(100.0, value)
		spin.set_value_no_signal(value)
	_units.select(maxi(UNITS.find(profile.get("units", "metric")), 0))
	_language.select(0)
	for i: int in range(LANGUAGES.size()):
		if LANGUAGES[i][0] == profile.get("language", ""):
			_language.select(i)
	var avatar: String = profile.get("avatar", RiderAvatar.RIDERS[0])
	_avatar.select(maxi(RiderAvatar.RIDERS.find(avatar), 0))
	_map_style.select(maxi(MAP_STYLES.find(profile.get("map_style", "coloured")), 0))
	var gradient: bool = profile.get("gradient_colours", false)
	_gradient_colours.select(1 if gradient else 0)
	_voice.select(maxi(VOICES.find(profile.get("voice", "casual")), 0))
	_calorie_sex.select(maxi(CALORIE_SEXES.find(profile.get("calorie_sex", "")), 0))
	_calorie_effort.select(maxi(CALORIE_EFFORTS.find(profile.get("calorie_effort", "")), 0))
	var drivetrain: String = profile.get("drivetrain", "single_cog")
	_drivetrain.select(maxi(DRIVETRAINS.find(drivetrain), 0))
	_show_teeth()


## The fields as `OrqwaApp.profile()` has them.
func profile() -> Dictionary:
	return {
		"rider_mass_kg": _rider_mass.value,
		"bike_mass_kg": _bike_mass.value,
		"ftp_w": _ftp.value,
		"max_heart_rate_bpm": _max_heart_rate.value,
		"default_difficulty_pct": _difficulty.value,
		"units": UNITS[_units.selected],
		"language": LANGUAGES[_language.selected][0],
		"avatar": RiderAvatar.RIDERS[_avatar.selected],
		"map_style": MAP_STYLES[_map_style.selected],
		"gradient_colours": _gradient_colours.selected == 1,
		"voice": VOICES[_voice.selected],
		"rides_per_week": roundi(_rides_per_week.value),
		"drivetrain": DRIVETRAINS[_drivetrain.selected],
		"chainring": _chainring.value,
		"cog": _cog.value,
		"calorie_age": roundi(_calorie_age.value),
		"calorie_age_year": _calorie_age_year,
		"calorie_sex": CALORIE_SEXES[_calorie_sex.selected],
		"calorie_effort": CALORIE_EFFORTS[_calorie_effort.selected],
	}


func ftp() -> float:
	return _ftp.value


func _on_calorie_age_changed(_age: float) -> void:
	_calorie_age_year = OrqwaApp.calendar_year()
	changed.emit()


func max_heart_rate() -> float:
	return _max_heart_rate.value


## Whether the rider has chosen miles and pounds.
func imperial() -> bool:
	return UNITS[_units.selected] == "imperial"


## Leaves out the trainer difficulty rides start with; during a ride the Ride tab sets the
## ride's own.
func hide_default_difficulty() -> void:
	for control: Control in _difficulty_row:
		control.hide()


func _show_teeth() -> void:
	for control: Control in _teeth_rows:
		control.visible = DRIVETRAINS[_drivetrain.selected] == "single_cog"


static func _spin(low: float, high: float, step: float, suffix: String) -> SpinBox:
	var spin: SpinBox = SpinBox.new()
	spin.min_value = low
	spin.max_value = high
	spin.step = step
	spin.suffix = suffix
	return spin
