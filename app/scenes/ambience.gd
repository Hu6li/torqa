class_name Ambience
extends Node
## The ride's ambient sound (R26): wind and tyre noise that follow the speed, rain, lapping water
## and birdsong where the road runs through forest by day. Everything is synthesised at start-up
## (noise loops, chirps) and shaped live by audio-bus filters, so there are no sound files to
## license.

const MIX_RATE: int = 22050
const LOOP_SECONDS: float = 2.0
## Speed at which wind and tyres reach full loudness.
const FULL_SPEED_KMH: float = 50.0
## How quickly loudness follows changes, per second.
const RESPONSE: float = 3.0
const BUS_PREFIX: String = "Ambience"

## Overall loudness 0–1; 0 silences the ambience.
var volume: float = 0.8
var _wind: AudioStreamPlayer = AudioStreamPlayer.new()
var _road: AudioStreamPlayer = AudioStreamPlayer.new()
var _rain: AudioStreamPlayer = AudioStreamPlayer.new()
var _water: AudioStreamPlayer = AudioStreamPlayer.new()
var _birds: AudioStreamPlayer = AudioStreamPlayer.new()
var _chirps: Array[AudioStreamWAV] = []
## Current linear loudness per loop, eased towards its target.
var _levels: Dictionary[AudioStreamPlayer, float] = {}
var _raining: bool = false
var _daylight: float = 1.0
var _next_bird: float = 2.0
var _wind_filter: AudioEffectLowPassFilter = AudioEffectLowPassFilter.new()


func _ready() -> void:
	var white: AudioStreamWAV = _noise(false)
	var brown: AudioStreamWAV = _noise(true)
	_wind_filter.cutoff_hz = 400.0
	var road_filter: AudioEffectBandPassFilter = AudioEffectBandPassFilter.new()
	road_filter.cutoff_hz = 220.0
	var rain_filter: AudioEffectHighPassFilter = AudioEffectHighPassFilter.new()
	rain_filter.cutoff_hz = 2500.0
	var water_filter: AudioEffectLowPassFilter = AudioEffectLowPassFilter.new()
	water_filter.cutoff_hz = 700.0
	for loop: Array in [
		[_wind, white, "Wind", _wind_filter],
		[_road, brown, "Road", road_filter],
		[_rain, white, "Rain", rain_filter],
		[_water, brown, "Water", water_filter],
	]:
		var player: AudioStreamPlayer = loop[0]
		var bus_name: String = loop[2]
		var effect: AudioEffect = loop[3]
		player.stream = loop[1]
		player.bus = _bus(bus_name, effect)
		player.volume_db = -80.0
		_levels[player] = 0.0
		add_child(player)
	for i: int in range(4):
		_chirps.append(_chirp(i))
	_birds.bus = _bus("Birds", null)
	_birds.max_polyphony = 3
	add_child(_birds)


## Rain and daylight from the ride's conditions (names as in `RideWorld.TIMES`/`WEATHERS`).
func set_conditions(time_of_day: String, weather: String) -> void:
	_raining = weather == "Rain"
	# Birds sing most in the morning, less at midday and in the evening.
	_daylight = {"Morning": 1.0, "Midday": 0.6, "Evening": 0.4}.get(time_of_day, 0.6)


## Follows the ride: `surroundings` as `ride_state()["surroundings"]` (null if unknown).
func update(delta: float, speed_kmh: float, surroundings: Variant) -> void:
	_start_loops()
	var forest: float = 0.0
	var water: float = 0.0
	var town: float = 0.0
	if surroundings != null:
		var around: Dictionary = surroundings
		forest = around["forest"]
		water = around["water"]
		town = around["town"]
	var pace: float = clampf(speed_kmh / FULL_SPEED_KMH, 0.0, 1.0)
	_wind_filter.cutoff_hz = lerpf(300.0, 2400.0, pace)
	_road.pitch_scale = lerpf(0.7, 1.4, pace)
	_ease(_wind, pow(pace, 1.5) * 0.55, delta)
	_ease(_road, minf(pace * 1.6, 1.0) * 0.22, delta)
	_ease(_rain, 0.45 if _raining else 0.0, delta)
	_ease(_water, water * 0.4, delta)

	_next_bird -= delta
	if _next_bird <= 0.0:
		_next_bird = randf_range(1.5, 6.0)
		var chance: float = (forest * 0.9 + 0.15) * (1.0 - town) * _daylight
		if not _raining and randf() < chance and volume > 0.0:
			_birds.stream = _chirps.pick_random()
			_birds.pitch_scale = randf_range(0.85, 1.25)
			# Wind noise at speed drowns birds out.
			_birds.volume_db = linear_to_db(0.35 * volume * (1.0 - pace * 0.6))
			_birds.play()


## Fades everything out, e.g. when the ride ends.
func silence() -> void:
	for player: AudioStreamPlayer in _levels:
		_levels[player] = 0.0
		player.volume_db = -80.0
		player.stop()
	_birds.stop()


func _start_loops() -> void:
	for player: AudioStreamPlayer in _levels:
		if not player.playing:
			player.play()


func _ease(player: AudioStreamPlayer, target: float, delta: float) -> void:
	var level: float = lerpf(_levels[player], target * volume, 1.0 - exp(-delta * RESPONSE))
	_levels[player] = level
	player.volume_db = linear_to_db(maxf(level, 0.0001))


## An audio bus feeding the master bus through `effect`, created once.
static func _bus(name: String, effect: AudioEffect) -> String:
	var bus_name: String = BUS_PREFIX + name
	if AudioServer.get_bus_index(bus_name) >= 0:
		return bus_name
	AudioServer.add_bus()
	var index: int = AudioServer.bus_count - 1
	AudioServer.set_bus_name(index, bus_name)
	AudioServer.set_bus_send(index, "Master")
	if effect != null:
		AudioServer.add_bus_effect(index, effect)
	return bus_name


## A seamless noise loop: white, or brown (integrated white noise, deeper and softer).
static func _noise(brown: bool) -> AudioStreamWAV:
	var count: int = roundi(MIX_RATE * LOOP_SECONDS)
	var data: PackedByteArray = PackedByteArray()
	data.resize(count * 2)
	var value: float = 0.0
	for i: int in range(count):
		var sample: float = randf_range(-1.0, 1.0)
		if brown:
			value = clampf(value * 0.98 + sample * 0.12, -1.0, 1.0)
			sample = value
		# The loop's ends fade into each other, so the seam is inaudible.
		var edge: float = minf(float(i), float(count - i)) / (MIX_RATE * 0.05)
		data.encode_s16(i * 2, roundi(sample * minf(edge, 1.0) * 20000.0))
	return _wav(data, true)


## A short call of two to four rising and falling syllables; `seed` varies it.
static func _chirp(seed: int) -> AudioStreamWAV:
	var random: RandomNumberGenerator = RandomNumberGenerator.new()
	random.seed = seed + 7
	var data: PackedByteArray = PackedByteArray()
	var phase: float = 0.0
	for syllable: int in range(random.randi_range(2, 4)):
		var length: int = roundi(MIX_RATE * random.randf_range(0.05, 0.11))
		var from_hz: float = random.randf_range(3200.0, 4200.0)
		var to_hz: float = random.randf_range(4200.0, 6000.0) * (1.0 if syllable % 2 == 0 else 0.8)
		var start: int = data.size()
		data.resize(start + length * 2)
		for i: int in range(length):
			var t: float = float(i) / length
			phase += TAU * lerpf(from_hz, to_hz, t) / MIX_RATE
			data.encode_s16(start + i * 2, roundi(sin(phase) * sin(PI * t) * 16000.0))
		var gap: int = roundi(MIX_RATE * random.randf_range(0.03, 0.07))
		data.resize(data.size() + gap * 2)
	return _wav(data, false)


static func _wav(data: PackedByteArray, loop: bool) -> AudioStreamWAV:
	var wav: AudioStreamWAV = AudioStreamWAV.new()
	wav.format = AudioStreamWAV.FORMAT_16_BITS
	wav.mix_rate = MIX_RATE
	wav.data = data
	if loop:
		wav.loop_mode = AudioStreamWAV.LOOP_FORWARD
		wav.loop_end = data.size() / 2
	return wav
