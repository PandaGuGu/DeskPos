using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace DeskPos
{
    // ---------------------------------------------------------------- 数据模型
    public class ScreenInfo
    {
        [JsonPropertyName("w")] public int W { get; set; }
        [JsonPropertyName("h")] public int H { get; set; }
    }

    public class IconPos
    {
        [JsonPropertyName("name")] public string Name { get; set; }
        [JsonPropertyName("x")] public int X { get; set; }
        [JsonPropertyName("y")] public int Y { get; set; }
    }

    public class LayoutFile
    {
        [JsonPropertyName("name")] public string Name { get; set; }
        [JsonPropertyName("created")] public string Created { get; set; }
        [JsonPropertyName("screen")] public ScreenInfo Screen { get; set; }
        [JsonPropertyName("auto_arrange")] public bool AutoArrange { get; set; }
        [JsonPropertyName("icons")] public List<IconPos> Icons { get; set; } = new List<IconPos>();
    }

    public class LayoutEntry
    {
        public string FileName { get; set; }
        public string Path { get; set; }
        public DateTime Stamp { get; set; }
        public LayoutFile Data { get; set; }
        public string DisplayName => Data?.Name ?? FileName;
        public int Count => Data?.Icons?.Count ?? 0;
        public string CountText => $"{Count} 个";
        public string Meta
        {
            get
            {
                string sc = Data?.Screen != null ? $"{Data.Screen.W}×{Data.Screen.H}" : "—";
                return $"{Stamp:MM-dd HH:mm}  ·  {sc}";
            }
        }
    }

    /// <summary>一次恢复的结果</summary>
    public class RestoreResult
    {
        /// <summary>最终与布局一致（按名字能对上）的图标数</summary>
        public int Restored;
        /// <summary>本次真正移动过的图标数</summary>
        public int Moved;
        /// <summary>重试轮数</summary>
        public int Rounds;
        /// <summary>是否用上了 shell 官方接口（能持久化）</summary>
        public bool UsedShellApi;
        /// <summary>恢复前是否帮你关掉了自动排列</summary>
        public bool AutoArrangeDisabled;
        /// <summary>布局里有、桌面上找不到的</summary>
        public List<string> Missing = new List<string>();
        /// <summary>桌面上有、布局里没有的（保持原位）</summary>
        public List<string> Extra = new List<string>();
        /// <summary>多轮重试后仍然摆不到位的</summary>
        public List<string> Failed = new List<string>();
    }

    // ---------------------------------------------------------------- Win32
    internal static class Native
    {
        public const int LVM_FIRST = 0x1000;
        public const int LVM_GETITEMCOUNT = LVM_FIRST + 4;
        public const int LVM_GETITEMPOSITION = LVM_FIRST + 16;
        public const int LVM_SETITEMPOSITION32 = LVM_FIRST + 49;
        public const int LVM_GETITEMTEXTW = LVM_FIRST + 115;
        public const int LVM_ARRANGE = LVM_FIRST + 22;
        public const int LVA_DEFAULT = 0x0000;

        public const int LVIF_TEXT = 0x0001;
        public const int LVS_AUTOARRANGE = 0x0100;
        public const int GWL_STYLE = -16;
        public const uint SMTO_ABORTIFHUNG = 0x0002;

        public const uint PROCESS_VM_OPERATION = 0x0008;
        public const uint PROCESS_VM_READ = 0x0010;
        public const uint PROCESS_VM_WRITE = 0x0020;
        public const uint PROCESS_QUERY_INFORMATION = 0x0400;
        public const uint MEM_COMMIT = 0x1000;
        public const uint MEM_RESERVE = 0x2000;
        public const uint MEM_RELEASE = 0x8000;
        public const uint PAGE_READWRITE = 0x04;

        public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr child, string cls, string title);

        [DllImport("user32.dll")]
        public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);

        [DllImport("user32.dll")]
        public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        public static extern IntPtr SendMessageTimeout(IntPtr hWnd, int msg, IntPtr wParam,
            IntPtr lParam, uint flags, uint timeout, out IntPtr result);

        [DllImport("user32.dll")]
        public static extern int GetWindowLong(IntPtr hWnd, int index);

        [DllImport("user32.dll")]
        public static extern int SetWindowLong(IntPtr hWnd, int index, int value);

        [DllImport("user32.dll")]
        public static extern int GetSystemMetrics(int index);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern IntPtr VirtualAllocEx(IntPtr proc, IntPtr addr, UIntPtr size,
            uint allocType, uint protect);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool VirtualFreeEx(IntPtr proc, IntPtr addr, UIntPtr size, uint freeType);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool ReadProcessMemory(IntPtr proc, IntPtr addr, byte[] buffer,
            UIntPtr size, out UIntPtr read);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool WriteProcessMemory(IntPtr proc, IntPtr addr, byte[] buffer,
            UIntPtr size, out UIntPtr written);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool CloseHandle(IntPtr h);

        [StructLayout(LayoutKind.Sequential)]
        public struct POINT { public int X; public int Y; }

        [StructLayout(LayoutKind.Sequential)]
        public struct LVITEMW
        {
            public uint mask;
            public int iItem;
            public int iSubItem;
            public uint state;
            public uint stateMask;
            public IntPtr pszText;
            public int cchTextMax;
            public int iImage;
            public IntPtr lParam;
            public int iIndent;
            public int iGroupId;
            public uint cColumns;
            public IntPtr puColumns;
            public IntPtr piColFmt;
            public int iGroup;
        }
    }

    // ---------------------------------------------------------------- 桌面图标访问
    public sealed class Desktop : IDisposable
    {
        private IntPtr _lv;
        private IntPtr _proc;
        private IntPtr _remote;
        private const int TEXT_OFF = 1024;

        public uint Pid { get; private set; }

        public Desktop()
        {
            _lv = FindListView();
            if (_lv == IntPtr.Zero)
                throw new InvalidOperationException("找不到桌面图标列表（explorer 未运行或桌面图标已隐藏）");

            Native.GetWindowThreadProcessId(_lv, out uint pid);
            Pid = pid;

            uint access = Native.PROCESS_VM_OPERATION | Native.PROCESS_VM_READ |
                          Native.PROCESS_VM_WRITE | Native.PROCESS_QUERY_INFORMATION;
            _proc = Native.OpenProcess(access, false, pid);
            if (_proc == IntPtr.Zero)
            {
                int err = Marshal.GetLastWin32Error();
                if (err == 5)
                    throw new InvalidOperationException("没有权限访问 explorer 进程，请用普通用户身份运行（不要用管理员）");
                throw new InvalidOperationException($"无法打开 explorer 进程（错误 {err}）");
            }

            _remote = Native.VirtualAllocEx(_proc, IntPtr.Zero, (UIntPtr)4096,
                Native.MEM_COMMIT | Native.MEM_RESERVE, Native.PAGE_READWRITE);
            if (_remote == IntPtr.Zero)
                throw new InvalidOperationException("在 explorer 中分配内存失败");
        }

        private static IntPtr FindListView()
        {
            IntPtr found = IntPtr.Zero;
            Native.EnumWindows((hwnd, l) =>
            {
                if (found != IntPtr.Zero) return true;
                IntPtr defView = Native.FindWindowEx(hwnd, IntPtr.Zero, "SHELLDLL_DefView", null);
                if (defView != IntPtr.Zero)
                {
                    IntPtr lv = Native.FindWindowEx(defView, IntPtr.Zero, "SysListView32", null);
                    if (lv != IntPtr.Zero) { found = lv; return false; }
                }
                return true;
            }, IntPtr.Zero);
            return found;
        }

        private IntPtr Send(int msg, IntPtr wParam, IntPtr lParam)
        {
            if (Native.SendMessageTimeout(_lv, msg, wParam, lParam,
                    Native.SMTO_ABORTIFHUNG, 3000, out IntPtr result) == IntPtr.Zero)
                throw new InvalidOperationException($"与桌面通讯超时（消息 0x{msg:X}）");
            return result;
        }

        private byte[] Read(IntPtr addr, int size)
        {
            byte[] buf = new byte[size];
            if (!Native.ReadProcessMemory(_proc, addr, buf, (UIntPtr)size, out _))
                throw new InvalidOperationException("读取 explorer 内存失败");
            return buf;
        }

        private void Write(IntPtr addr, byte[] data)
        {
            if (!Native.WriteProcessMemory(_proc, addr, data, (UIntPtr)data.Length, out _))
                throw new InvalidOperationException("写入 explorer 内存失败");
        }

        public int Count => (int)Send(Native.LVM_GETITEMCOUNT, IntPtr.Zero, IntPtr.Zero);

        public bool AutoArrange =>
            (Native.GetWindowLong(_lv, Native.GWL_STYLE) & Native.LVS_AUTOARRANGE) != 0;

        public bool DisableAutoArrange()
        {
            int style = Native.GetWindowLong(_lv, Native.GWL_STYLE);
            if ((style & Native.LVS_AUTOARRANGE) == 0) return false;
            // 只清样式位，不要发 LVM_ARRANGE——那会立刻把所有图标重排一遍，
            // 反而把刚写好的位置冲掉（这也是以前「要点好几次」的原因之一）
            Native.SetWindowLong(_lv, Native.GWL_STYLE, style & ~Native.LVS_AUTOARRANGE);
            return true;
        }

        public string ItemName(int index)
        {
            IntPtr textAddr = _remote + TEXT_OFF;
            var item = new Native.LVITEMW
            {
                mask = Native.LVIF_TEXT,
                iItem = 0,
                iSubItem = 0,
                pszText = textAddr,
                cchTextMax = 260,
            };
            Write(_remote, StructToBytes(item));
            Send(Native.LVM_GETITEMTEXTW, (IntPtr)index, _remote);
            byte[] raw = Read(textAddr, 520);
            string s = Encoding.Unicode.GetString(raw);
            int z = s.IndexOf('\0');
            return z >= 0 ? s.Substring(0, z) : s;
        }

        public (int x, int y) ItemPos(int index)
        {
            Write(_remote, new byte[8]);
            Send(Native.LVM_GETITEMPOSITION, (IntPtr)index, _remote);
            var pt = BytesToStruct<Native.POINT>(Read(_remote, 8));
            return (pt.X, pt.Y);
        }

        public void SetPos(int index, int x, int y)
        {
            var pt = new Native.POINT { X = x, Y = y };
            Write(_remote, StructToBytes(pt));
            Send(Native.LVM_SETITEMPOSITION32, (IntPtr)index, _remote);
        }

        public List<IconPos> Snapshot()
        {
            int n = Count;
            var list = new List<IconPos>(n);
            for (int i = 0; i < n; i++)
            {
                var (x, y) = ItemPos(i);
                list.Add(new IconPos { Name = ItemName(i), X = x, Y = y });
            }
            return list;
        }

        /// <summary>返回 (要移动的图标数, 已消失, 新增)</summary>
        public (int move, List<string> missing, List<string> extra, int total) Plan(LayoutFile data)
        {
            var saved = new Dictionary<string, IconPos>();
            foreach (var it in data.Icons) saved[it.Name] = it;

            var current = new Dictionary<string, int>();
            int n = Count;
            for (int i = 0; i < n; i++) current[ItemName(i)] = i;

            var missing = new List<string>();
            var extra = new List<string>();
            int move = 0;
            foreach (var kv in saved)
            {
                if (current.TryGetValue(kv.Key, out int idx))
                {
                    var (cx, cy) = ItemPos(idx);
                    if (cx != kv.Value.X || cy != kv.Value.Y) move++;
                }
                else missing.Add(kv.Key);
            }
            foreach (var k in current.Keys) if (!saved.ContainsKey(k)) extra.Add(k);
            return (move, missing, extra, saved.Count);
        }

        /// <summary>
        /// 恢复布局。
        /// 优先走 shell 的 IFolderView::SelectAndPositionItems——位置由 shell 自己设置，
        /// 它会同步更新 Bags\Desktop\IconLayouts，重启后依然保持；
        /// shell 接口不可用时退回内存写入，并做多轮校验。
        /// </summary>
        public RestoreResult Restore(LayoutFile data)
        {
            bool autoOff = AutoArrange && DisableAutoArrange();

            var saved = new Dictionary<string, IconPos>(StringComparer.OrdinalIgnoreCase);
            foreach (var it in data.Icons)
                if (!string.IsNullOrEmpty(it.Name)) saved[it.Name] = it;

            var result = new RestoreResult { AutoArrangeDisabled = autoOff };

            // 以当前桌面的图标名为基准算 missing / extra
            var current = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            int n = Count;
            for (int i = 0; i < n; i++) current.Add(ItemName(i));
            foreach (var k in saved.Keys) if (!current.Contains(k)) result.Missing.Add(k);
            foreach (var k in current) if (!saved.ContainsKey(k)) result.Extra.Add(k);

            // ---------- 首选：shell 官方接口 ----------
            try
            {
                using (ShellView sv = ShellView.TryOpen())
                {
                    if (sv != null)
                    {
                        var (moved, failed, rounds) = sv.Apply(saved);
                        result.UsedShellApi = true;
                        result.Moved = moved;
                        result.Rounds = rounds;
                        result.Failed = failed;
                        result.Restored = saved.Count - result.Missing.Count - failed.Count;
                        return result;
                    }
                }
            }
            catch
            {
                // 忽略，落到内存方式
            }

            // ---------- 退路：内存写入 + 多轮校验 ----------
            var (restored, memMoved, memFailed) = RestoreByMemory(saved);
            result.Restored = restored;
            result.Moved = memMoved;
            result.Failed = memFailed;
            return result;
        }

        /// <summary>
        /// 老办法：LVM_SETITEMPOSITION32 直接改 explorer 内存。
        /// explorer 的网格吸附是异步的，写一次不一定全生效，所以每轮重读、
        /// 只补写还有偏差的图标，最多 4 轮。
        /// </summary>
        private (int restored, int moved, List<string> failed) RestoreByMemory(Dictionary<string, IconPos> saved)
        {
            int moved = 0;
            for (int round = 0; round < 4; round++)
            {
                int n = Count;
                int wrote = 0;
                for (int i = 0; i < n; i++)
                {
                    string name = ItemName(i);
                    if (!saved.TryGetValue(name, out IconPos want)) continue;
                    var (cx, cy) = ItemPos(i);
                    if (cx == want.X && cy == want.Y) continue;
                    SetPos(i, want.X, want.Y);
                    wrote++;
                    if (round == 0) moved++;
                }
                if (wrote == 0) break;
                System.Threading.Thread.Sleep(140);
            }

            // 核对
            var failed = new List<string>();
            int ok = 0;
            int total = Count;
            for (int i = 0; i < total; i++)
            {
                string name = ItemName(i);
                if (!saved.TryGetValue(name, out IconPos want)) continue;
                var (cx, cy) = ItemPos(i);
                if (cx == want.X && cy == want.Y) ok++;
                else failed.Add(name);
            }
            return (ok, moved, failed);
        }

        private static byte[] StructToBytes<T>(T value) where T : struct
        {
            int size = Marshal.SizeOf<T>();
            byte[] arr = new byte[size];
            IntPtr p = Marshal.AllocHGlobal(size);
            try
            {
                Marshal.StructureToPtr(value, p, false);
                Marshal.Copy(p, arr, 0, size);
            }
            finally { Marshal.FreeHGlobal(p); }
            return arr;
        }

        private static T BytesToStruct<T>(byte[] data) where T : struct
        {
            IntPtr p = Marshal.AllocHGlobal(data.Length);
            try
            {
                Marshal.Copy(data, 0, p, data.Length);
                return Marshal.PtrToStructure<T>(p);
            }
            finally { Marshal.FreeHGlobal(p); }
        }

        public void Dispose()
        {
            if (_remote != IntPtr.Zero)
            {
                Native.VirtualFreeEx(_proc, _remote, UIntPtr.Zero, Native.MEM_RELEASE);
                _remote = IntPtr.Zero;
            }
            if (_proc != IntPtr.Zero)
            {
                Native.CloseHandle(_proc);
                _proc = IntPtr.Zero;
            }
        }
    }

    // ---------------------------------------------------------------- 布局存储
    public static class Store
    {
        public static string Dir
        {
            get
            {
                string baseDir = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
                string path = Path.Combine(baseDir, "DesktopIconPos", "layouts");
                Directory.CreateDirectory(path);
                return path;
            }
        }

        private static readonly JsonSerializerOptions Opts = new JsonSerializerOptions
        {
            WriteIndented = true,
            Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        };

        public static string PathOf(string name) => Path.Combine(Dir, Sanitize(name) + ".json");

        private static string Sanitize(string n)
        {
            foreach (char c in "<>:\"/\\|?*") n = n.Replace(c, '_');
            n = n.Trim();
            return n.Length == 0 ? "layout" : n;
        }

        public static void Save(LayoutFile layout)
        {
            File.WriteAllText(PathOf(layout.Name), JsonSerializer.Serialize(layout, Opts));
        }

        public static List<LayoutEntry> List()
        {
            var list = new List<LayoutEntry>();
            foreach (string f in Directory.GetFiles(Dir, "*.json"))
            {
                try
                {
                    var data = JsonSerializer.Deserialize<LayoutFile>(File.ReadAllText(f));
                    if (data == null) continue;
                    list.Add(new LayoutEntry
                    {
                        FileName = Path.GetFileNameWithoutExtension(f),
                        Path = f,
                        Stamp = File.GetLastWriteTime(f),
                        Data = data,
                    });
                }
                catch { /* 跳过损坏的文件 */ }
            }
            list.Sort((a, b) => b.Stamp.CompareTo(a.Stamp));
            return list;
        }

        public static void Delete(LayoutEntry e)
        {
            if (File.Exists(e.Path)) File.Delete(e.Path);
        }
    }
}
