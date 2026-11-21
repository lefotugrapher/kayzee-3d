Kayzee 3D for Windows (64-bit, Windows 10 or 11)
================================================

Run
  Double-click Kayzee3D.exe. Nothing to install.
  Windows may show a "Windows protected your PC" notice because the
  program is not signed: choose "More info", then "Run anyway".

First run: unlock rendering
  Everything works without it, but rendering needs your own Bryce 3D disc
  image. Choose File > Add Bryce 3D Disc Image... and pick your .iso file
  (File > Add From Disc or Folder... takes a disc or an installed copy).
  This is done once. The data is kept in  %APPDATA%\Kayzee 3D  and never
  leaves the PC.

If the window opens blank or without text
  The window is drawn with OpenGL by default. To try the other way, make
  a shortcut to Kayzee3D.exe and add  --wgpu  to its Target, or run:
      Kayzee3D.exe --wgpu

Files
  Scenes you save are .ridge files. The app opens .br3 and .br5 scenes
  and imports OBJ, DXF, 3DS and 3DMF models. Movies are saved as .avi
  (Motion JPEG); pictures as PNG, BMP, TIFF or JPEG.

Keys
  Where the Mac version uses the Command key, use Ctrl; for Option use Alt.
  Help > Keys and Mouse lists them.
