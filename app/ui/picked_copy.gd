class_name PickedCopy
extends RefCounted
## Copies a picked Android file into ORQWA's folder ([method PickedFile.copy_of]) on a thread of
## its own. A world runs to gigabytes: copied on the main thread, the app drew nothing for
## minutes and Android showed it see-through.

## How far the copy is, in bytes. On the main thread.
signal progressed(done: int, total: int)
## The copy's path; "" if it failed or was cancelled. On the main thread.
signal finished(path: String)

var _thread: Thread = Thread.new()
var _mutex: Mutex = Mutex.new()
var _cancelled: bool = false


## Starts copying `picked`, a content address.
func start(picked: String) -> void:
	_thread.start(_copy.bind(picked, PickedFile.name_of(picked).get_file()))


## Stops the copy and removes what was written; [signal finished] follows with "".
func cancel() -> void:
	_mutex.lock()
	_cancelled = true
	_mutex.unlock()


## Cancels and waits for the thread, for an owner that goes away before the copy is done.
func stop() -> void:
	cancel()
	if _thread.is_started():
		_thread.wait_to_finish()


func _copy(picked: String, file_name: String) -> void:
	var path: String = PickedFile.copy_of(picked, file_name, _on_chunk)
	_finish.call_deferred(path)


func _on_chunk(done: int, total: int) -> bool:
	progressed.emit.call_deferred(done, total)
	_mutex.lock()
	var go_on: bool = not _cancelled
	_mutex.unlock()
	return go_on


func _finish(path: String) -> void:
	if _thread.is_started():
		_thread.wait_to_finish()
	finished.emit(path)
