Kayzee 3D for Linux (64-bit x86, glibc 2.28 or newer)
=====================================================

Run
  tar xzf Kayzee3D-Linux.tar.gz
  cd Kayzee3D-Linux
  ./kayzee3d

  Needs a desktop with X11 or Wayland and OpenGL (any current distro does).
  File dialogs use your desktop's portal (xdg-desktop-portal), which GNOME,
  KDE and most desktops already have.

First run: unlock rendering
  Everything works without it, but rendering needs your own Bryce 3D disc
  image. Choose File > Add Bryce 3D Disc Image... and pick your .iso file.
  The data is kept in ~/.local/share/Kayzee 3D and never leaves the machine.

If the window stays blank, try:  ./kayzee3d --wgpu

Movies are saved as .avi (Motion JPEG). Report problems at
https://github.com/lefotugrapher/kayzee-3d/issues
