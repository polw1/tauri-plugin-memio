project('memio-webkit-extension', 'c', default_options: ['c_std=c11'])

webkit = dependency('webkit2gtk-4.1', required: false)
if not webkit.found()
  webkit = dependency('webkit2gtk-4.0')
endif

shared_library(
  'memio_web_extension',
  'extension.c',
  dependencies: [webkit],
  install: false,
  name_prefix: ''
)
