class_name PickedFile
extends RefCounted
## A file the rider picked, as ORQWA's core can open or write it. On Android the file pickers
## hand over a content address (`content://…`) that only Android's own file access reads and
## writes (ADR 0024): a picked file is copied into ORQWA's folder first, and one to save is
## written there by the core, then copied to the address. Elsewhere paths stay as they are.

const COPIES: String = "user://picked"
## ORQWA's Android plugin (ADR 0024), there only on Android: it names picked files the address
## itself does not name.
const ANDROID: StringName = &"OrqwaAndroid"


## The filters for a file dialog that opens one of `filters`' files: none on Android. Its picker
## turns each extension into a MIME type and greys out files stored under another one, which in
## Recent and Downloads is whatever the download was typed as (a GPX as application/gpx+xml,
## say): the core goes by the picked file's name instead.
static func open_filters(filters: PackedStringArray) -> PackedStringArray:
	return PackedStringArray() if OS.has_feature("android") else filters


## Why ORQWA does not open `picked`, which the rider meant as one of `extensions` (lower case,
## without the dot); "" if it has one of them. Checked by name before the file is copied or
## read, so a file that is something else is never handed to a reader for another format.
static func refusal(picked: String, extensions: PackedStringArray) -> String:
	var file: String = name_of(picked).get_file()
	if file.get_extension().to_lower() in extensions:
		return ""
	var dotted: PackedStringArray = PackedStringArray()
	for extension: String in extensions:
		dotted.append("." + extension)
	return (
		TranslationServer.translate("ORQWA cannot open %s: it opens %s files.")
		% [file, ", ".join(dotted)]
	)


## The path to give the core for `picked`; `picked` itself if an Android file cannot be copied,
## so the core reports it as unreadable as before.
static func local_path(picked: String) -> String:
	if not picked.begins_with("content://"):
		return picked
	var data: PackedByteArray = FileAccess.get_file_as_bytes(picked)
	if data.is_empty():
		return picked
	DirAccess.make_dir_recursive_absolute(COPIES)
	var copy: String = COPIES.path_join(name_of(picked))
	var file: FileAccess = FileAccess.open(copy, FileAccess.WRITE)
	if file == null:
		return picked
	file.store_buffer(data)
	file.close()
	return ProjectSettings.globalize_path(copy)


## Where the core should write a file the rider wants saved at `picked`; then [method save] it.
static func save_path(picked: String) -> String:
	if not picked.begins_with("content://"):
		return picked
	DirAccess.make_dir_recursive_absolute(COPIES)
	return ProjectSettings.globalize_path(COPIES.path_join(name_of(picked)))


## Copies what the core wrote at `written` (from [method save_path]) to `picked`; whether that
## worked. Nothing to copy outside Android.
static func save(written: String, picked: String) -> bool:
	if written == picked:
		return true
	var data: PackedByteArray = FileAccess.get_file_as_bytes(written)
	var file: FileAccess = FileAccess.open(picked, FileAccess.WRITE)
	if data.is_empty() or file == null:
		return false
	file.store_buffer(data)
	file.close()
	DirAccess.remove_absolute(written)
	return true


## How to name `picked` to the rider: the file's own name rather than a content address. One
## ending in the file's path answers itself (…%3ADownload%2FRide.gpx); one ending in a document
## id (…/document/msf:1000003599) names nothing, and the file's extension decides what the core
## makes of it: Android is asked for the display name first.
static func name_of(picked: String) -> String:
	if not picked.begins_with("content://"):
		return picked
	return _content_name(picked, _display_name(picked))


## What a content `address`'s file is called: the `display_name` its provider gives Android
## when it answers one, else the address's own last part, else "picked".
static func _content_name(address: String, display_name: String) -> String:
	var file_name: String = display_name.get_file()
	if file_name.is_empty():
		file_name = address.uri_decode().get_file()
	return file_name if not file_name.is_empty() else "picked"


## The picked file's display name as Android knows it; "" anywhere but on Android, or when its
## provider names nothing.
static func _display_name(address: String) -> String:
	if not Engine.has_singleton(ANDROID):
		return ""
	var named: Variant = Engine.get_singleton(ANDROID).call(&"pickedName", address)
	return named if named is String else ""
