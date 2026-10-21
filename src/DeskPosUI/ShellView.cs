using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

namespace DeskPos
{
    // ================================================================
    // 通过 shell 自己的 IFolderView 设置图标位置。
    //
    // 为什么不直接写 explorer 内存：
    //   1) shell 的文件夹视图内部状态没变，explorer 一刷新就把位置纠回去，
    //      所以以前要点好几次才收敛；
    //   2) Win10 1809+ shell 用注册表 Bags\1\Desktop\IconLayouts 保存位置，
    //      内存改写不会写进这个值，重启后被旧值覆盖 → 布局全乱。
    //   IFolderView::SelectAndPositionItems 是官方接口，shell 会自己更新
    //   内部状态并立即持久化（实测重启 explorer 后位置保持）。
    // ================================================================

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X; public int Y; }

    [ComImport, Guid("9BA05972-F6A8-11CF-A442-00A0C90A8F39")]
    internal class ShellWindowsClass { }

    [ComImport, Guid("85CB6900-4D95-11CF-960C-0080C7F4EE85"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IShellWindows
    {
        [PreserveSig] int GetTypeInfoCount(out uint pctinfo);
        [PreserveSig] int GetTypeInfo(uint iTInfo, uint lcid, out IntPtr ppTInfo);
        [PreserveSig] int GetIDsOfNames(ref Guid riid, IntPtr[] rgszNames, uint cNames, uint lcid, IntPtr rgDispId);
        [PreserveSig] int Invoke(int dispIdMember, ref Guid riid, uint lcid, ushort wFlags,
            IntPtr pDispParams, IntPtr pVarResult, IntPtr pExcepInfo, IntPtr puArgErr);

        [PreserveSig] int get_Count(out int count);
        [PreserveSig] int Item(IntPtr index, out IntPtr ppid);
        [PreserveSig] int _NewEnum(out IntPtr ppenum);
        [PreserveSig] int Register(IntPtr pid, int hWnd, int swClass, out int plCookie);
        [PreserveSig] int RegisterPending(int lThreadId, IntPtr pvarloc, IntPtr pvarlocRoot, int swClass, out int plCookie);
        [PreserveSig] int Revoke(int lCookie);
        [PreserveSig] int OnNavigate(int lCookie, IntPtr pvarLoc);
        [PreserveSig] int OnActivated(int lCookie, IntPtr pvarLoc, IntPtr pvarLocRoot, int fActive);
        [PreserveSig] int FindWindowSW(IntPtr pvarLoc, IntPtr pvarLocRoot, int swClass,
            out int hWnd, int swfwOptions, out IntPtr ppdispOut);
        [PreserveSig] int OnCreated(int lCookie);
        [PreserveSig] int ProcessAttachDetach(int fAttach);
    }

    [ComImport, Guid("6d5140c1-7436-11ce-8034-00aa006009fa"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IServiceProvider
    {
        [PreserveSig] int QueryService(ref Guid guidService, ref Guid riid, out IntPtr ppvObject);
    }

    [ComImport, Guid("000214E2-0000-0000-C000-000000000046"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IShellBrowser
    {
        [PreserveSig] int GetWindow(out IntPtr hwnd);
        [PreserveSig] int ContextSensitiveHelp(int fEnterMode);
        [PreserveSig] int InsertMenusSB(IntPtr hmenuShared, IntPtr lpMenuWidths);
        [PreserveSig] int SetMenuSB(IntPtr hmenuShared, IntPtr holemenuRes, IntPtr hwndActiveObject);
        [PreserveSig] int RemoveMenusSB(IntPtr hmenuShared);
        [PreserveSig] int SetStatusTextSB(IntPtr pszStatusText);
        [PreserveSig] int EnableModelessSB(int fEnable);
        [PreserveSig] int TranslateAcceleratorSB(IntPtr pmsg, ushort wID);
        [PreserveSig] int BrowseObject(IntPtr pidl, uint wFlags);
        [PreserveSig] int GetViewStateStream(uint grfMode, out IntPtr ppStrm);
        [PreserveSig] int GetControlWindow(uint id, out IntPtr phwnd);
        [PreserveSig] int SendControlMsg(uint id, uint uMsg, IntPtr wParam, IntPtr lParam, out IntPtr pret);
        [PreserveSig] int QueryActiveShellView(out IntPtr ppshv);
        [PreserveSig] int OnViewWindowActive(IntPtr pshv);
        [PreserveSig] int SetToolbarItems(IntPtr lpButtons, uint nButtons, uint uFlags);
    }

    [ComImport, Guid("cde725b0-ccc9-4519-917e-325d72fab4ce"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IFolderView
    {
        [PreserveSig] int GetCurrentViewMode(out uint pViewMode);
        [PreserveSig] int SetCurrentViewMode(uint ViewMode);
        [PreserveSig] int GetFolder(ref Guid riid, out IntPtr ppv);
        [PreserveSig] int Item(int iItemIndex, out IntPtr ppidl);
        [PreserveSig] int ItemCount(uint uFlags, out int pcItems);
        [PreserveSig] int Items(uint uFlags, ref Guid riid, out IntPtr ppv);
        [PreserveSig] int GetSelectionMarkedItem(out int piItem);
        [PreserveSig] int GetFocusedItem(out int piItem);
        [PreserveSig] int GetItemPosition(IntPtr pidl, out POINT ppt);
        [PreserveSig] int GetSpacing(out POINT ppt);
        [PreserveSig] int GetDefaultSpacing(out POINT ppt);
        [PreserveSig] int GetAutoArrange();
        [PreserveSig] int SelectItem(int iItem, uint dwFlags);
        [PreserveSig] int SelectAndPositionItems(uint cidl,
            [MarshalAs(UnmanagedType.LPArray, SizeParamIndex = 0)] IntPtr[] apidl,
            [MarshalAs(UnmanagedType.LPArray, SizeParamIndex = 0)] POINT[] apt,
            uint dwFlags);
    }

    [ComImport, Guid("000214E6-0000-0000-C000-000000000046"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IShellFolder
    {
        [PreserveSig] int ParseDisplayName(IntPtr hwnd, IntPtr pbc,
            [MarshalAs(UnmanagedType.LPWStr)] string pszDisplayName,
            out uint pchEaten, out IntPtr ppidl, ref uint pdwAttributes);
        [PreserveSig] int EnumObjects(IntPtr hwnd, uint grfFlags, out IntPtr ppenumIDList);
        [PreserveSig] int BindToObject(IntPtr pidl, IntPtr pbc, ref Guid riid, out IntPtr ppv);
        [PreserveSig] int BindToStorage(IntPtr pidl, IntPtr pbc, ref Guid riid, out IntPtr ppv);
        [PreserveSig] int CompareIDs(IntPtr lParam, IntPtr pidl1, IntPtr pidl2);
        [PreserveSig] int CreateViewObject(IntPtr hwndOwner, ref Guid riid, out IntPtr ppv);
        [PreserveSig] int GetAttributesOf(uint cidl,
            [MarshalAs(UnmanagedType.LPArray)] IntPtr[] apidl, ref uint rgfInOut);
        [PreserveSig] int GetUIObjectOf(IntPtr hwndOwner, uint cidl,
            [MarshalAs(UnmanagedType.LPArray)] IntPtr[] apidl, ref Guid riid,
            IntPtr rgfReserved, out IntPtr ppv);
        [PreserveSig] int GetDisplayNameOf(IntPtr pidl, uint uFlags, IntPtr pName);
        [PreserveSig] int SetNameOf(IntPtr hwnd, IntPtr pidl,
            [MarshalAs(UnmanagedType.LPWStr)] string pszName, uint uFlags, out IntPtr ppidlOut);
    }

    /// <summary>桌面视图的一个条目</summary>
    public sealed class ShellItem
    {
        public string Name;
        public IntPtr Pidl;
        public int X;
        public int Y;
    }

    /// <summary>用 shell 官方接口读写桌面图标位置，失败时返回 null 由调用方回退到内存方式</summary>
    public sealed class ShellView : IDisposable
    {
        private const int SWC_DESKTOP = 8;
        private const int SWFO_NEEDDISPATCH = 1;
        private const uint SVSI_POSITIONITEM = 0x0080;
        private const uint SHGDN_INFOLDER = 0x0001;

        private static readonly Guid SID_STopLevelBrowser = new Guid("4C96BE40-915C-11CF-99D3-00AA004AE837");
        private static readonly Guid IID_IShellBrowser = new Guid("000214E2-0000-0000-C000-000000000046");
        private static readonly Guid IID_IFolderView = new Guid("cde725b0-ccc9-4519-917e-325d72fab4ce");

        [DllImport("shell32.dll")]
        private static extern int SHGetDesktopFolder(out IShellFolder ppshf);
        [DllImport("shlwapi.dll", CharSet = CharSet.Unicode)]
        private static extern int StrRetToBufW(IntPtr pstr, IntPtr pidl, StringBuilder pszBuf, uint cchBuf);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        private static extern IntPtr FindWindowEx(IntPtr parent, IntPtr child, string cls, string title);
        [DllImport("user32.dll")]
        private static extern bool EnumWindows(Native.EnumWindowsProc cb, IntPtr lParam);
        [DllImport("user32.dll")]
        private static extern bool SetProcessDpiAwarenessContext(IntPtr value);

        private IFolderView _fv;
        private IShellFolder _desk;
        private int _hwnd;
        private readonly List<ShellItem> _items = new List<ShellItem>();

        public int Count => _items.Count;
        public IReadOnlyList<ShellItem> Items => _items;
        public int ViewHandle => _hwnd;

        /// <summary>找桌面的 IFolderView，拿不到返回 null（例如非交互会话）</summary>
        public static ShellView TryOpen()
        {
            var sv = new ShellView();
            if (!sv.Open()) { sv.Dispose(); return null; }
            sv.Reload();
            if (sv.Count == 0) { sv.Dispose(); return null; }
            return sv;
        }

        private bool Open()
        {
            try
            {
                var win = (IShellWindows)new ShellWindowsClass();
                IntPtr pvarLoc = Marshal.AllocCoTaskMem(24);
                IntPtr pvarRoot = Marshal.AllocCoTaskMem(24);
                for (int i = 0; i < 24; i++) { Marshal.WriteByte(pvarLoc, i, 0); Marshal.WriteByte(pvarRoot, i, 0); }
                Marshal.WriteInt16(pvarLoc, 0, 3);   // vt = VT_I4
                Marshal.WriteInt32(pvarLoc, 8, 0);   // CSIDL_DESKTOP

                int hr;
                IntPtr disp;
                try { hr = win.FindWindowSW(pvarLoc, pvarRoot, SWC_DESKTOP, out _hwnd, SWFO_NEEDDISPATCH, out disp); }
                finally { Marshal.FreeCoTaskMem(pvarLoc); Marshal.FreeCoTaskMem(pvarRoot); }
                if (hr != 0 || disp == IntPtr.Zero) return false;

                try
                {
                    var sp = (IServiceProvider)Marshal.GetTypedObjectForIUnknown(disp, typeof(IServiceProvider));
                    Guid sid = SID_STopLevelBrowser, iid = IID_IShellBrowser;
                    hr = sp.QueryService(ref sid, ref iid, out IntPtr pBrowser);
                    if (hr != 0 || pBrowser == IntPtr.Zero) return false;

                    try
                    {
                        var browser = (IShellBrowser)Marshal.GetTypedObjectForIUnknown(pBrowser, typeof(IShellBrowser));
                        hr = browser.QueryActiveShellView(out IntPtr pView);
                        if (hr != 0 || pView == IntPtr.Zero) return false;

                        Guid fvIid = IID_IFolderView;
                        hr = Marshal.QueryInterface(pView, ref fvIid, out IntPtr pFv);
                        Marshal.Release(pView);
                        if (hr != 0 || pFv == IntPtr.Zero) return false;

                        _fv = (IFolderView)Marshal.GetTypedObjectForIUnknown(pFv, typeof(IFolderView));
                    }
                    finally { Marshal.Release(pBrowser); }
                }
                finally { Marshal.Release(disp); }

                if (SHGetDesktopFolder(out _desk) != 0) return false;
                return true;
            }
            catch
            {
                return false;
            }
        }

        /// <summary>重新枚举条目：名字取自 shell 的显示名，与布局里保存的完全一致</summary>
        public void Reload()
        {
            ReleasePidls();
            _items.Clear();
            if (_fv == null) return;

            if (_fv.ItemCount(0, out int n) != 0) return;

            IntPtr strret = Marshal.AllocCoTaskMem(264);
            try
            {
                for (int i = 0; i < n; i++)
                {
                    if (_fv.Item(i, out IntPtr pidl) != 0 || pidl == IntPtr.Zero) continue;
                    if (_fv.GetItemPosition(pidl, out POINT pt) != 0) { Marshal.FreeCoTaskMem(pidl); continue; }

                    string name = null;
                    if (_desk != null && _desk.GetDisplayNameOf(pidl, SHGDN_INFOLDER, strret) == 0)
                    {
                        var sb = new StringBuilder(260);
                        if (StrRetToBufW(strret, pidl, sb, 260) == 0) name = sb.ToString();
                    }
                    if (string.IsNullOrEmpty(name)) name = $"#{i}";

                    _items.Add(new ShellItem { Name = name, Pidl = pidl, X = pt.X, Y = pt.Y });
                }
            }
            finally { Marshal.FreeCoTaskMem(strret); }
        }

        public ShellItem Find(string name)
        {
            foreach (var it in _items)
                if (string.Equals(it.Name, name, StringComparison.OrdinalIgnoreCase)) return it;
            return null;
        }

        /// <summary>
        /// 批量设置位置。返回 (移动数, 最终仍不一致的图标名)。
        /// shell 的吸附是异步的，所以写完要读回核对，不一致就再来一轮。
        /// </summary>
        public (int moved, List<string> failed, int rounds) Apply(Dictionary<string, IconPos> saved, int maxRounds = 4)
        {
            int moved = 0, rounds = 0;
            for (int round = 0; round < maxRounds; round++)
            {
                Reload();
                var apidl = new List<IntPtr>();
                var apt = new List<POINT>();

                foreach (var it in _items)
                {
                    if (!saved.TryGetValue(it.Name, out IconPos want)) continue;
                    if (it.X == want.X && it.Y == want.Y) continue;
                    apidl.Add(it.Pidl);
                    apt.Add(new POINT { X = want.X, Y = want.Y });
                }
                if (apidl.Count == 0) break;

                rounds++;
                if (round == 0) moved = apidl.Count;
                int hr = _fv.SelectAndPositionItems((uint)apidl.Count, apidl.ToArray(), apt.ToArray(), SVSI_POSITIONITEM);
                if (hr != 0) throw new InvalidOperationException($"设置图标位置失败（0x{hr:X8}）");
                Thread.Sleep(160);
            }

            Reload();
            var failed = new List<string>();
            foreach (var it in _items)
            {
                if (!saved.TryGetValue(it.Name, out IconPos want)) continue;
                if (it.X != want.X || it.Y != want.Y) failed.Add(it.Name);
            }
            return (moved, failed, rounds);
        }

        private void ReleasePidls()
        {
            foreach (var it in _items)
                if (it.Pidl != IntPtr.Zero) Marshal.FreeCoTaskMem(it.Pidl);
            _items.Clear();
        }

        public void Dispose()
        {
            ReleasePidls();
            if (_desk != null) { Marshal.ReleaseComObject(_desk); _desk = null; }
            if (_fv != null) { Marshal.ReleaseComObject(_fv); _fv = null; }
        }
    }
}
