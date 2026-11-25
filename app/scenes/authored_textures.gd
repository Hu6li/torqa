class_name AuthoredTextures
extends RefCounted
## The images a world file stores once for all its glTF files to share (`textures`,
## docs/world-files.md): each read from the file and decoded once, on a worker thread, however
## many sections draw it. Kept here only until the sections waiting for it are made; their
## materials hold it then, and a weak reference finds it again for the next section.

var _ask: Callable
var _threaded: bool
## Textures decoded and still wanted by a section that waits: entry → texture.
var _ready: Dictionary[String, Texture2D] = {}
## Entries the file could not give or Godot could not decode: their sections go ahead without.
var _failed: Dictionary[String, bool] = {}
## Entry → weak reference to its texture, once nothing waits for it any more.
var _kept: Dictionary[String, WeakRef] = {}
## Decoding on worker threads: entry → task id; and the textures made: entry → texture.
var _decoding: Dictionary[String, int] = {}
var _decoded: Dictionary[String, Texture2D] = {}
var _decoded_lock: Mutex = Mutex.new()


## `ask` requests an entry of the world file; `threaded` decodes on worker threads.
func _init(ask: Callable, threaded: bool) -> void:
	_ask = ask
	_threaded = threaded


## The shared images a `.glb`, the world file's entry `file`, draws: image index → entry.
static func shared(file: String, bytes: PackedByteArray) -> Dictionary[int, String]:
	var found: Dictionary[int, String] = {}
	var gltf: Dictionary = AuthoredMaterials.glb_json(bytes)
	var images: Array = gltf.get("images", [])
	# Images only extensions use are never read: this ORQWA draws without what it does not know.
	var drawn: Dictionary[int, bool] = AuthoredMaterials.drawn_images(gltf)
	for index: int in images.size():
		if not drawn.has(index):
			continue
		if not images[index] is Dictionary:
			continue
		var image: Dictionary = images[index]
		if not image.get("uri") is String:
			continue
		var uri: String = image["uri"]
		found[index] = file.get_base_dir().path_join(uri).simplify_path()
	return found


## Whether every entry of `entries` is decoded or failed; asks for those not yet asked.
func have(entries: Array[String]) -> bool:
	var all: bool = true
	for entry: String in entries:
		if _ready.has(entry) or _failed.has(entry):
			continue
		var kept: Texture2D = _kept_texture(entry)
		if kept != null:
			_ready[entry] = kept
			continue
		all = false
		if not _decoding.has(entry):
			_ask.call(entry)
	return all


## The textures for `entries`, all decoded ([method have]): entry → texture, null for one that
## failed.
func textures(entries: Array[String]) -> Dictionary[String, Texture2D]:
	var found: Dictionary[String, Texture2D] = {}
	for entry: String in entries:
		found[entry] = _ready.get(entry)
	return found


## Takes an entry the core read: its image, or why it could not be read.
func take(entry: String, part: Variant) -> void:
	if not part is PackedByteArray:
		push_warning("world file texture %s: %s" % [entry, part])
		_failed[entry] = true
		return
	var bytes: PackedByteArray = part
	if _threaded:
		_decoding[entry] = WorkerThreadPool.add_task(
			_decode.bind(entry, bytes, _decoded, _decoded_lock)
		)
	else:
		_decode(entry, bytes, _decoded, _decoded_lock)
		_decoding[entry] = -1


## Collects what the workers decoded.
func collect() -> void:
	for entry: String in _decoding.keys():
		var task: int = _decoding[entry]
		if task >= 0:
			if not WorkerThreadPool.is_task_completed(task):
				continue
			WorkerThreadPool.wait_for_task_completion(task)
		_decoding.erase(entry)
		_decoded_lock.lock()
		var texture: Texture2D = _decoded.get(entry)
		_decoded.erase(entry)
		_decoded_lock.unlock()
		if texture == null:
			push_warning("world file texture %s is not an image Godot can read" % entry)
			_failed[entry] = true
		else:
			_ready[entry] = texture


## Lets go of the textures no section waits for any more (`needed`): the sections made hold
## those they draw.
func release(needed: Dictionary[String, bool]) -> void:
	for entry: String in _ready.keys():
		if not needed.has(entry):
			_kept[entry] = weakref(_ready[entry])
			_ready.erase(entry)


## Waits for the workers and lets go of everything: the world is closed.
func clear() -> void:
	for task: int in _decoding.values():
		if task >= 0:
			WorkerThreadPool.wait_for_task_completion(task)
	_decoding.clear()
	_decoded.clear()
	_ready.clear()
	_failed.clear()
	_kept.clear()


func _kept_texture(entry: String) -> Texture2D:
	var reference: WeakRef = _kept.get(entry)
	if reference == null:
		return null
	var texture: Texture2D = reference.get_ref()
	if texture == null:
		_kept.erase(entry)
	return texture


## Runs on a worker thread: the texture of an image, no larger than the preset allows, with
## mipmaps. A cut-out's mipmaps are made again on the main thread when a section uses it.
static func _decode(
	entry: String,
	bytes: PackedByteArray,
	decoded: Dictionary[String, Texture2D],
	decoded_lock: Mutex,
) -> void:
	var image: Image = Image.new()
	var error: Error = ERR_FILE_UNRECOGNIZED
	match entry.get_extension().to_lower():
		"png":
			error = image.load_png_from_buffer(bytes)
		"jpg", "jpeg":
			error = image.load_jpg_from_buffer(bytes)
		"webp":
			error = image.load_webp_from_buffer(bytes)
	var texture: Texture2D = null
	if error == OK and not image.is_empty():
		AuthoredMaterials.cap(image)
		image.generate_mipmaps()
		texture = ImageTexture.create_from_image(image)
	decoded_lock.lock()
	decoded[entry] = texture
	decoded_lock.unlock()
