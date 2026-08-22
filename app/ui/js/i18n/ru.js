// Russian catalogue (#50).
//
// Agreed terminology, to be kept consistent as the rest of the interface
// arrives: stage → «подготовить», tag block → «блок тегов», placeholder →
// «подстановка», library → «библиотека». Also тег, маска, папка, обложка,
// дубликат, релиз, правило, цепочка.
//
// Partial by design — only the panels that have been translated so far are
// here. Every missing key falls back to English, and an English string keeps
// English plural rules while it does, so nothing reads as broken in between.
//
// Russian selects between `one` / `few` / `many` for a count — 1 трек,
// 2 трека, 5 треков; 21 is `one` again and 11 is `many`. `tn` asks
// `Intl.PluralRules` rather than guessing, so a form is never picked by hand.
export const ru = {
  // ---- units that appear inside other messages ----
  "unit.track": { one: "{n} трек", few: "{n} трека", many: "{n} треков" },
  "unit.file": { one: "{n} файл", few: "{n} файла", many: "{n} файлов" },
  "unit.playlist": { one: "{n} плейлист", few: "{n} плейлиста", many: "{n} плейлистов" },

  // ---- EXPORTER ----
  "exporter.heading": "Экспорт",
  "exporter.format": "Формат",
  "exporter.format.playlist": "Плейлист",
  "exporter.format.cue": "CUE",
  "exporter.format.csv": "CSV",
  "exporter.format.html": "HTML",
  "exporter.format.xml": "XML",
  "exporter.format.report": "Отчёт",
  "exporter.format.aria": "Формат экспорта",
  "exporter.hint.playlist": "Плейлист <b>.m3u</b> из выбранных треков, в порядке таблицы.",
  "exporter.hint.cue": "Лист <b>.cue</b> — один <b>FILE</b> на трек, нумерация в порядке таблицы.",
  "exporter.hint.csv": "По <b>строке на трек</b> с колонками тегов — открывается в любой таблице.",
  "exporter.hint.html":
    "Самодостаточная <b>HTML-таблица</b> с колонками тегов — открывается в любом браузере.",
  "exporter.hint.xml":
    "<b>XML-документ</b> — по элементу на тег, для скриптов и других инструментов.",
  "exporter.hint.report": "Каждый трек, отрисованный по <b>маске</b> ниже, по строке на трек.",
  "exporter.hint.split": "По одному <b>.m3u</b> на {grouping}, имя по маске ниже.",
  "exporter.split": "По одному на",
  "exporter.split.selection": "Выделение",
  "exporter.split.folder": "Папку",
  "exporter.split.album": "Альбом",
  "exporter.grouping.folder": "папку",
  "exporter.grouping.album": "альбом",
  "exporter.mask": "Маска",
  "exporter.mask.placeholders": "Подстановки",
  "exporter.mask.placeholdersAria": "Справочник подстановок",
  "exporter.name": "Имя файла",
  "exporter.nameMask": "Маска имени",
  "exporter.note":
    "<b>Только чтение.</b> Записывается в открытую папку библиотеки — ваши аудиофайлы не изменяются.",
  "exporter.run": "Экспортировать",
  "toast.export.selectFirst": "Сначала выберите треки для экспорта",
  "toast.export.done": "Экспортировано {tracks} в {file}",
  "toast.export.playlists": "Экспортировано {playlists}",

  // ---- Settings › Display ----
  "settings.language": "Язык",
  "settings.language.aria": "Язык интерфейса",
  "settings.language.auto": "Авто",
  "settings.language.en": "English",
  "settings.language.ru": "Русский",
  "settings.language.hint":
    "Авто следует языку системы. Язык, для которого нет каталога, откатывается на английский.",
};
