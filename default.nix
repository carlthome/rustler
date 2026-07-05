{
  pkgs,
  ...
}:

pkgs.rustPlatform.buildRustPackage {
  pname = "rustler";
  version = "0.1.0";
  src = ./.;
  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = with pkgs; [
    pkg-config
    lldb
    makeWrapper
  ];

  buildInputs =
    with pkgs;
    lib.optionals stdenv.isLinux [
      glib
      gtk3
      cairo
      pango
      gdk-pixbuf
      glibc
      xorg.libX11
      xorg.libXcursor
      xorg.libXrandr
      xorg.libXi
      xorg.libXext
      xorg.libXinerama
      xorg.libXxf86vm
      xorg.libXrender
      xorg.libxcb
      xorg.libXau
      xorg.libXdmcp
      mesa
      alsa-lib
      dbus
      freetype
      fontconfig
      zlib
      udev
      wayland
      wayland-protocols
      libxkbcommon
      vulkan-loader
    ];
  shellHook =
    with pkgs;
    lib.optionalString stdenv.isLinux ''
      export LD_LIBRARY_PATH="${lib.makeLibraryPath [
        wayland
        xorg.libxcb
        vulkan-loader
        libxkbcommon
      ]}:/run/opengl-driver/lib:$LD_LIBRARY_PATH"
      export XDG_DATA_DIRS="/run/opengl-driver/share:$XDG_DATA_DIRS"
      export VK_ICD_FILENAMES="/run/opengl-driver/share/vulkan/icd.d/nvidia_icd.json"
    '';
  postInstall =
    with pkgs;
    ''
      cp -r resources $out/bin
    ''
    + lib.optionalString stdenv.isLinux ''
      wrapProgram $out/bin/rustler \
        --prefix LD_LIBRARY_PATH : "${
          lib.makeLibraryPath [
            wayland
            xorg.libxcb
            vulkan-loader
            libxkbcommon
          ]
        }:/run/opengl-driver/lib" \
        --prefix XDG_DATA_DIRS : "/run/opengl-driver/share" \
        --set-default VK_ICD_FILENAMES "/run/opengl-driver/share/vulkan/icd.d/nvidia_icd.json"
    '';
}
