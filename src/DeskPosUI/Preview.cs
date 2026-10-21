using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace DeskPos
{
    /// <summary>按布局里保存的真实坐标，等比绘制一块「缩小的桌面」。</summary>
    public sealed class DeskPreview : FrameworkElement
    {
        private LayoutFile _data;
        private List<IconPos> _ghost;
        private readonly List<Hit> _hits = new List<Hit>();
        private string _hover;

        private struct Hit
        {
            public Rect Rect;
            public string Name;
        }

        public event EventHandler<string> HoverChanged;

        public DeskPreview()
        {
            RenderOptions.SetBitmapScalingMode(this, BitmapScalingMode.HighQuality);
            ClipToBounds = true;
        }

        public LayoutFile Data
        {
            get => _data;
            set
            {
                _data = value;
                _hits.Clear();
                SetHover(null);
                IconCache.Refresh();
                Wallpaper.Refresh();
                InvalidateVisual();
            }
        }

        /// <summary>叠加显示当前桌面的图标位置（灰白描边）。</summary>
        public List<IconPos> Ghost
        {
            get => _ghost;
            set { _ghost = value; InvalidateVisual(); }
        }

        // ------------------------------------------------------------ 交互
        protected override void OnMouseMove(System.Windows.Input.MouseEventArgs e)
        {
            var p = e.GetPosition(this);
            string found = null;
            foreach (var h in _hits)
            {
                if (h.Rect.Contains(p)) { found = h.Name; break; }
            }
            SetHover(found);
            base.OnMouseMove(e);
        }

        protected override void OnMouseLeave(System.Windows.Input.MouseEventArgs e)
        {
            SetHover(null);
            base.OnMouseLeave(e);
        }

        private void SetHover(string name)
        {
            if (_hover == name) return;
            _hover = name;
            HoverChanged?.Invoke(this, name);
            InvalidateVisual();
        }

        // ------------------------------------------------------------ 绘制
        protected override void OnRender(DrawingContext dc)
        {
            double w = ActualWidth, h = ActualHeight;
            if (w < 4 || h < 4) return;
            _hits.Clear();

            // 背板
            var back = new SolidColorBrush(Color.FromRgb(0x0C, 0x0E, 0x13));
            back.Freeze();
            dc.DrawRoundedRectangle(back, null, new Rect(0, 0, w, h), 8, 8);

            var data = _data;
            if (data == null || data.Icons == null || data.Icons.Count == 0) return;

            int sw = data.Screen != null && data.Screen.W > 0 ? data.Screen.W : 1920;
            int sh = data.Screen != null && data.Screen.H > 0 ? data.Screen.H : 1080;

            // 图标框尺寸（100% 缩放下约 56px），并按坐标范围扩展画布，兼容负坐标副屏
            const double BOX = 50;
            double minX = 0, minY = 0, maxX = sw, maxY = sh;
            foreach (var it in data.Icons)
            {
                if (it.X < minX) minX = it.X;
                if (it.Y < minY) minY = it.Y;
                if (it.X + BOX > maxX) maxX = it.X + BOX;
                if (it.Y + BOX > maxY) maxY = it.Y + BOX;
            }

            double vw = Math.Max(1, maxX - minX), vh = Math.Max(1, maxY - minY);
            const double PAD = 9;
            double scale = Math.Min((w - PAD * 2) / vw, (h - PAD * 2) / vh);
            double ox = (w - vw * scale) / 2 - minX * scale;
            double oy = (h - vh * scale) / 2 - minY * scale;

            Point P(double x, double y) => new Point(ox + x * scale, oy + y * scale);

            var screenRect = new Rect(P(0, 0), P(sw, sh));

            var clip = new RectangleGeometry(screenRect, 6, 6);
            clip.Freeze();
            dc.PushClip(clip);

            DrawWallpaper(dc, screenRect);

            // 当前桌面位置（幽灵）
            if (_ghost != null && _ghost.Count > 0)
            {
                var pen = new Pen(new SolidColorBrush(Color.FromArgb(0xE6, 0x6F, 0x8C, 0xFF)), 1.3)
                {
                    DashStyle = new DashStyle(new double[] { 2.2, 1.8 }, 0),
                };
                pen.Freeze();
                var fill = new SolidColorBrush(Color.FromArgb(0x24, 0x10, 0x18, 0x30));
                fill.Freeze();
                foreach (var g in _ghost)
                {
                    var r = new Rect(P(g.X, g.Y), P(g.X + BOX, g.Y + BOX));
                    dc.DrawRoundedRectangle(fill, pen, r, 3, 3);
                }
            }

            // 图标
            double box = BOX * scale;
            foreach (var it in data.Icons)
            {
                var r = new Rect(P(it.X, it.Y), P(it.X + BOX, it.Y + BOX));
                var hit = new Rect(r.X, r.Y, box, box);
                _hits.Add(new Hit { Rect = hit, Name = it.Name });

                if (_hover == it.Name)
                {
                    var hb = new SolidColorBrush(Color.FromArgb(0x30, 0x5B, 0x7C, 0xFF));
                    hb.Freeze();
                    var hp = new Pen(new SolidColorBrush(Color.FromArgb(0x99, 0x7E, 0x9B, 0xFF)), 1);
                    hp.Freeze();
                    dc.DrawRoundedRectangle(hb, hp, hit, 4, 4);
                }

                var img = IconCache.Get(it.Name);
                if (img != null)
                {
                    dc.DrawImage(img, hit);
                }
                else
                {
                    // 找不到图标时用首字色块兜底
                    var c = IconCache.ColorOf(it.Name);
                    var fill = new SolidColorBrush(c);
                    fill.Freeze();
                    dc.DrawRoundedRectangle(fill, null, hit, 3, 3);
                    var ch = new FormattedText(
                        it.Name.Substring(0, 1), CultureInfo.CurrentUICulture, FlowDirection.LeftToRight,
                        new Typeface("Microsoft YaHei UI"), Math.Max(7, box * 0.62), Brushes.White,
                        VisualTreeHelper.GetDpi(this).PixelsPerDip)
                    {
                        MaxTextWidth = box,
                        TextAlignment = TextAlignment.Center,
                    };
                    dc.DrawText(ch, new Point(hit.X, hit.Y + (box - ch.Height) / 2));
                }
            }

            // 任务栏
            DrawTaskbar(dc, screenRect, scale, P);

            // 屏幕边框
            var edge = new Pen(new SolidColorBrush(Color.FromArgb(0x66, 0x64, 0x74, 0x94)), 1);
            edge.Freeze();
            dc.DrawRoundedRectangle(null, edge, screenRect, 6, 6);

            dc.Pop();

            // 悬停名称气泡
            if (_hover != null)
            {
                DrawTooltip(dc, _hover, w, h);
            }
        }

        private void DrawWallpaper(DrawingContext dc, Rect r)
        {
            var img = Wallpaper.Current;
            if (img != null)
            {
                var brush = new ImageBrush(img) { Stretch = Stretch.UniformToFill };
                brush.Freeze();
                dc.DrawRectangle(brush, null, r);

                // 压一点点亮度，避免缩略图里图标看不清
                var scrim = new SolidColorBrush(Color.FromArgb(0x16, 0x08, 0x0A, 0x10));
                scrim.Freeze();
                dc.DrawRectangle(scrim, null, r);
                return;
            }

            var wb = new LinearGradientBrush { StartPoint = new Point(0, 0), EndPoint = new Point(1, 1) };
            wb.GradientStops.Add(new GradientStop(Color.FromRgb(0x17, 0x25, 0x45), 0));
            wb.GradientStops.Add(new GradientStop(Color.FromRgb(0x23, 0x1B, 0x3E), 0.52));
            wb.GradientStops.Add(new GradientStop(Color.FromRgb(0x11, 0x17, 0x2A), 1));
            wb.Freeze();
            dc.DrawRectangle(wb, null, r);

            var glow = new RadialGradientBrush();
            glow.GradientStops.Add(new GradientStop(Color.FromArgb(0x3A, 0x6C, 0x9C, 0xFF), 0));
            glow.GradientStops.Add(new GradientStop(Color.FromArgb(0x00, 0x6C, 0x9C, 0xFF), 1));
            glow.Center = new Point(0.26, 0.14);
            glow.GradientOrigin = new Point(0.26, 0.14);
            glow.RadiusX = 0.75;
            glow.RadiusY = 0.95;
            glow.Freeze();
            dc.DrawRectangle(glow, null, r);

            var glow2 = new RadialGradientBrush();
            glow2.GradientStops.Add(new GradientStop(Color.FromArgb(0x2A, 0xA8, 0x6C, 0xFF), 0));
            glow2.GradientStops.Add(new GradientStop(Color.FromArgb(0x00, 0xA8, 0x6C, 0xFF), 1));
            glow2.Center = new Point(0.82, 0.88);
            glow2.GradientOrigin = new Point(0.82, 0.88);
            glow2.RadiusX = 0.6;
            glow2.RadiusY = 0.8;
            glow2.Freeze();
            dc.DrawRectangle(glow2, null, r);
        }

        private void DrawTaskbar(DrawingContext dc, Rect r, double scale, Func<double, double, Point> P)
        {
            double tb = Math.Max(4, 52 * scale);
            var bar = new Rect(r.X, r.Bottom - tb, r.Width, tb);
            var bg = new SolidColorBrush(Color.FromArgb(0xF0, 0x0A, 0x0D, 0x14));
            bg.Freeze();
            dc.DrawRectangle(bg, null, bar);

            var line = new Pen(new SolidColorBrush(Color.FromArgb(0x55, 0x4A, 0x58, 0x76)), Math.Max(0.6, scale));
            line.Freeze();
            dc.DrawLine(line, new Point(bar.X, bar.Y), new Point(bar.Right, bar.Y));

            // 开始按钮
            double s = tb * 0.42;
            double cy = bar.Y + tb / 2;
            var start = new Rect(bar.X + tb * 0.28, cy - s / 2, s, s);
            var sg = new LinearGradientBrush(Color.FromRgb(0x5B, 0x7C, 0xFF), Color.FromRgb(0x8F, 0x5C, 0xFF), 45);
            sg.Freeze();
            dc.DrawRoundedRectangle(sg, null, start, s * 0.22, s * 0.22);

            // 搜索框
            double sx = start.Right + tb * 0.34;
            var search = new Rect(sx, cy - tb * 0.19, r.Width * 0.155, tb * 0.38);
            var sf = new SolidColorBrush(Color.FromArgb(0x1E, 0xFF, 0xFF, 0xFF));
            sf.Freeze();
            dc.DrawRoundedRectangle(sf, null, search, tb * 0.16, tb * 0.16);

            // 固定在任务栏的程序
            double ax = search.Right + tb * 0.5;
            for (int i = 0; i < 4; i++)
            {
                double asz = tb * 0.4;
                var a = new Rect(ax + i * tb * 0.62, cy - asz / 2, asz, asz);
                if (a.Right > r.Right - tb * 3.4) break;
                var af = new SolidColorBrush(Color.FromArgb(i == 0 ? (byte)0x40 : (byte)0x1C, 0xFF, 0xFF, 0xFF));
                af.Freeze();
                dc.DrawRoundedRectangle(af, null, a, asz * 0.2, asz * 0.2);
            }

            // 时间 / 通知
            double rx = r.Right - tb * 0.42;
            var clock = new Rect(rx - r.Width * 0.045, cy - tb * 0.19, r.Width * 0.045, tb * 0.38);
            var cf = new SolidColorBrush(Color.FromArgb(0x1A, 0xFF, 0xFF, 0xFF));
            cf.Freeze();
            dc.DrawRoundedRectangle(cf, null, clock, tb * 0.16, tb * 0.16);
        }

        private void DrawTooltip(DrawingContext dc, string name, double w, double h)
        {
            var dpi = VisualTreeHelper.GetDpi(this).PixelsPerDip;
            var ft = new FormattedText(name, CultureInfo.CurrentUICulture, FlowDirection.LeftToRight,
                new Typeface("Microsoft YaHei UI"), 10.5, Brushes.White, dpi)
            {
                MaxTextWidth = 220,
                Trimming = TextTrimming.CharacterEllipsis,
            };

            double tw = Math.Min(ft.Width, 220) + 14;
            double th = ft.Height + 8;

            // 定位到悬停图标附近
            Rect anchor = new Rect(w / 2, h / 2, 1, 1);
            foreach (var it in _hits)
            {
                if (it.Name == name) { anchor = it.Rect; break; }
            }

            double x = anchor.X + anchor.Width / 2 - tw / 2;
            double y = anchor.Bottom + 4;
            if (y + th > h - 4) y = anchor.Y - th - 4;
            x = Math.Max(4, Math.Min(x, w - tw - 4));
            y = Math.Max(4, Math.Min(y, h - th - 4));

            var pill = new Rect(x, y, tw, th);
            var fill = new SolidColorBrush(Color.FromArgb(0xF2, 0x1B, 0x20, 0x2C));
            fill.Freeze();
            var pen = new Pen(new SolidColorBrush(Color.FromArgb(0xAA, 0x3E, 0x49, 0x60)), 1);
            pen.Freeze();
            dc.DrawRoundedRectangle(fill, pen, pill, 5, 5);
            dc.DrawText(ft, new Point(pill.X + 7, pill.Y + (th - ft.Height) / 2));
        }
    }

    // ---------------------------------------------------------------- 壁纸
    internal static class Wallpaper
    {
        private const uint SPI_GETDESKWALLPAPER = 0x0073;
        private static ImageSource _img;
        private static bool _loaded;

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        private static extern bool SystemParametersInfo(uint action, uint param, StringBuilder data, uint winIni);

        public static ImageSource Current
        {
            get
            {
                if (_loaded) return _img;
                _loaded = true;
                try
                {
                    var sb = new StringBuilder(520);
                    if (!SystemParametersInfo(SPI_GETDESKWALLPAPER, (uint)sb.Capacity, sb, 0)) return null;

                    string path = sb.ToString();
                    if (string.IsNullOrWhiteSpace(path) || !File.Exists(path)) return null;

                    var bmp = new BitmapImage();
                    bmp.BeginInit();
                    bmp.CacheOption = BitmapCacheOption.OnLoad;
                    bmp.CreateOptions = BitmapCreateOptions.IgnoreImageCache;
                    bmp.UriSource = new Uri(path, UriKind.Absolute);
                    bmp.DecodePixelWidth = 900;
                    bmp.EndInit();
                    bmp.Freeze();
                    _img = bmp;
                }
                catch { _img = null; }
                return _img;
            }
        }

        public static void Refresh() => _loaded = false;
    }

    // ---------------------------------------------------------------- 图标提取
    internal static class IconCache
    {
        private const uint SHGFI_ICON = 0x000000100;
        private const uint SHGFI_LARGEICON = 0x000000000;
        private const uint SHGFI_PIDL = 0x000000008;

        private static readonly Dictionary<string, ImageSource> Cache =
            new Dictionary<string, ImageSource>(StringComparer.OrdinalIgnoreCase);

        private static Dictionary<string, string> _bare;

        private static readonly string[] Dirs =
        {
            Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory),
            Environment.GetFolderPath(Environment.SpecialFolder.CommonDesktopDirectory),
        };

        // 桌面上的系统虚拟项（shell 命名空间路径）
        private static readonly Dictionary<string, string> Virtual = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            ["此电脑"] = "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}",
            ["This PC"] = "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}",
            ["计算机"] = "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}",
            ["回收站"] = "::{645FF040-5081-101B-9F08-00AA002F954E}",
            ["Recycle Bin"] = "::{645FF040-5081-101B-9F08-00AA002F954E}",
            ["网络"] = "::{F02C1A0D-BE21-4350-88B0-7367FC96EF3C}",
            ["Network"] = "::{F02C1A0D-BE21-4350-88B0-7367FC96EF3C}",
            ["控制面板"] = "::{5399E694-6CE5-4D6C-8FCE-1D8870FDCBA0}",
            ["Control Panel"] = "::{5399E694-6CE5-4D6C-8FCE-1D8870FDCBA0}",
            ["用户文件夹"] = "::{59031A47-3F72-44A7-89C5-5595FE6B30EE}",
            ["Administrator"] = "::{59031A47-3F72-44A7-89C5-5595FE6B30EE}",
            ["Administrator 的文件夹"] = "::{59031A47-3F72-44A7-89C5-5595FE6B30EE}",
        };

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct SHFILEINFO
        {
            public IntPtr hIcon;
            public int iIcon;
            public uint dwAttributes;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szDisplayName;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 80)] public string szTypeName;
        }

        [DllImport("shell32.dll", CharSet = CharSet.Unicode, EntryPoint = "SHGetFileInfoW")]
        private static extern IntPtr SHGetFileInfo(string path, uint attr, ref SHFILEINFO info, uint size, uint flags);

        [DllImport("shell32.dll", EntryPoint = "SHGetFileInfoW")]
        private static extern IntPtr SHGetFileInfoPidl(IntPtr pidl, uint attr, ref SHFILEINFO info, uint size, uint flags);

        [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
        private static extern int SHParseDisplayName(string name, IntPtr bindCtx, out IntPtr pidl, uint sfgaoIn, out uint sfgaoOut);

        [DllImport("shell32.dll")]
        private static extern void ILFree(IntPtr pidl);

        [DllImport("user32.dll")]
        private static extern bool DestroyIcon(IntPtr hIcon);

        /// <summary>桌面内容变了就调一下，重新扫描文件名。</summary>
        public static void Refresh()
        {
            _bare = null;
        }

        private static Dictionary<string, string> Bare()
        {
            if (_bare != null) return _bare;
            var map = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            foreach (string dir in Dirs)
            {
                try
                {
                    if (string.IsNullOrEmpty(dir) || !Directory.Exists(dir)) continue;
                    foreach (string f in Directory.GetFiles(dir))
                    {
                        string bn = Path.GetFileNameWithoutExtension(f);
                        if (!map.ContainsKey(bn)) map[bn] = f;
                        string fn = Path.GetFileName(f);
                        if (!map.ContainsKey(fn)) map[fn] = f;
                    }
                }
                catch { }
            }
            _bare = map;
            return _bare;
        }

        public static ImageSource Get(string name)
        {
            if (string.IsNullOrEmpty(name)) return null;
            if (Cache.TryGetValue(name, out var cached)) return cached;

            ImageSource img = null;

            // 1. 系统虚拟项
            if (Virtual.TryGetValue(name, out string shellPath)) img = LoadShell(shellPath);

            // 2. 桌面上的实体文件 / 文件夹
            if (img == null)
            {
                foreach (string dir in Dirs)
                {
                    if (string.IsNullOrEmpty(dir)) continue;
                    string p = Path.Combine(dir, name);
                    if (File.Exists(p) || Directory.Exists(p)) { img = Load(p); break; }
                }
            }

            // 3. 短名匹配（快捷方式、隐藏扩展名）
            if (img == null && Bare().TryGetValue(name, out string hit)) img = Load(hit);

            Cache[name] = img;
            return img;
        }

        private static ImageSource Load(string path)
        {
            try
            {
                var info = new SHFILEINFO();
                IntPtr ok = SHGetFileInfo(path, 0, ref info,
                    (uint)Marshal.SizeOf<SHFILEINFO>(), SHGFI_ICON | SHGFI_LARGEICON);
                return Convert(info, ok);
            }
            catch { return null; }
        }

        private static ImageSource LoadShell(string shellPath)
        {
            IntPtr pidl = IntPtr.Zero;
            try
            {
                uint attrs;
                if (SHParseDisplayName(shellPath, IntPtr.Zero, out pidl, 0, out attrs) != 0 || pidl == IntPtr.Zero)
                    return null;

                var info = new SHFILEINFO();
                IntPtr ok = SHGetFileInfoPidl(pidl, 0, ref info,
                    (uint)Marshal.SizeOf<SHFILEINFO>(), SHGFI_ICON | SHGFI_LARGEICON | SHGFI_PIDL);
                return Convert(info, ok);
            }
            catch { return null; }
            finally { if (pidl != IntPtr.Zero) ILFree(pidl); }
        }

        private static ImageSource Convert(SHFILEINFO info, IntPtr ok)
        {
            if (ok == IntPtr.Zero || info.hIcon == IntPtr.Zero) return null;
            try
            {
                var src = Imaging.CreateBitmapSourceFromHIcon(info.hIcon, Int32Rect.Empty,
                    BitmapSizeOptions.FromEmptyOptions());
                src.Freeze();
                return src;
            }
            finally { DestroyIcon(info.hIcon); }
        }

        public static Color ColorOf(string name)
        {
            int h = 0;
            foreach (char c in name ?? "") h = (h * 31 + c) & 0x7FFFFFFF;
            double hue = h % 360;
            return Hsv(hue, 0.55, 0.62);
        }

        private static Color Hsv(double h, double s, double v)
        {
            double c = v * s;
            double x = c * (1 - Math.Abs((h / 60) % 2 - 1));
            double m = v - c;
            double r = 0, g = 0, b = 0;
            if (h < 60) { r = c; g = x; }
            else if (h < 120) { r = x; g = c; }
            else if (h < 180) { g = c; b = x; }
            else if (h < 240) { g = x; b = c; }
            else if (h < 300) { r = x; b = c; }
            else { r = c; b = x; }
            return Color.FromRgb((byte)((r + m) * 255), (byte)((g + m) * 255), (byte)((b + m) * 255));
        }
    }
}
