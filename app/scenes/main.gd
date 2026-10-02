extends Control

@onready var _version_label: Label = $VersionLabel


func _ready() -> void:
	var title: String = "Torqa %s" % TorqaCore.version()
	_version_label.text = title
	print(title)
