# Applies SNAPCAP_SANITIZE (e.g. "address,undefined" or "thread") to everything built after this point.
if(SNAPCAP_SANITIZE)
  if(MSVC)
    message(FATAL_ERROR "SNAPCAP_SANITIZE is only supported with Clang/GCC")
  endif()
  add_compile_options(-fsanitize=${SNAPCAP_SANITIZE} -fno-omit-frame-pointer -fno-sanitize-recover=all)
  add_link_options(-fsanitize=${SNAPCAP_SANITIZE})
endif()
