using System.Diagnostics;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Threading;

namespace DeskPos
{
    public partial class MainWindow : Window
    {
        private readonly DispatcherTimer _toastTimer;
        private TaskCompletionSource<bool> _tcs;
        private bool _dialogHasInput;
        private bool _showGhost;

        public MainWindow()
        {
            InitializeComponent();

            Preview.HoverChanged += (s, name) =>
                PreviewHover.Text = string.IsNullOrEmpty(name) ? "悬停图标可查看名称" : name;

            _toastTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(2400) };
            _toastTimer.Tick += (s, e) =>
            {
                _toastTimer.Stop();
                var fade = new DoubleAnimation(1, 0, TimeSpan.FromMilliseconds(220));
                Toast.BeginAnimation(OpacityProperty, fade);
            };

            Loaded += (s, e) => Refresh();
        }

        // ------------------------------------------------------------ 刷新
        private void Refresh()
        {
            try
            {
                using (var d = new Desktop())
                {
                    StatIcons.Text = d.Count.ToString();
                    bool auto = d.AutoArrange;
                    StatArrange.Text = auto ? "已开启" : "已关闭";
                    StatArrange.Foreground = auto ? new SolidColorBrush(Color.FromRgb(0xF0, 0x9B, 0x4B))
                                                  : new SolidColorBrush(Color.FromRgb(0x3D, 0xDC, 0x97));
                    DotArrange.Fill = auto ? new SolidColorBrush(Color.FromRgb(0xF0, 0x9B, 0x4B))
                                           : new SolidColorBrush(Color.FromRgb(0x3D, 0xDC, 0x97));
                    StatScreen.Text = $"{Native.GetSystemMetrics(0)}×{Native.GetSystemMetrics(1)}";
                    StatusText.Text = auto
                        ? "提示：桌面开启了「自动排列图标」，恢复前 DeskPos 会自动帮你关掉"
                        : "就绪 · 已连接到桌面";
                }
            }
            catch (Exception ex)
            {
                StatIcons.Text = "—";
                StatArrange.Text = "—";
                StatScreen.Text = "—";
                StatusText.Text = ex.Message;
            }

            string keep = (LayoutList.SelectedItem as LayoutEntry)?.FileName;
            var items = Store.List();
            LayoutList.ItemsSource = items;

            if (items.Count == 0)
            {
                EmptyHint.Visibility = Visibility.Visible;
                LayoutList.Visibility = Visibility.Collapsed;
            }
            else
            {
                EmptyHint.Visibility = Visibility.Collapsed;
                LayoutList.Visibility = Visibility.Visible;
                LayoutEntry pick = keep != null ? items.FirstOrDefault(x => x.FileName == keep) : null;
                LayoutList.SelectedItem = pick ?? items[0];
            }
            UpdateButtons();
            ShowPreview();
        }

        // ------------------------------------------------------------ 预览
        private void ShowPreview()
        {
            var entry = Selected;
            bool has = entry?.Data != null;

            PreviewEmpty.Visibility = has ? Visibility.Collapsed : Visibility.Visible;
            PreviewTitle.Text = has ? entry.DisplayName : "";
            PreviewHover.Text = "悬停图标可查看名称";

            if (has)
            {
                var s = entry.Data.Screen;
                PreviewMeta.Text = $"{entry.Count} 个图标 · " + (s != null ? $"{s.W}×{s.H}" : "分辨率未知");
            }
            else
            {
                PreviewMeta.Text = "";
            }

            Preview.Data = has ? entry.Data : null;
            if (_showGhost) LoadGhost();

            if (has)
            {
                Preview.BeginAnimation(OpacityProperty,
                    new DoubleAnimation(0.4, 1, TimeSpan.FromMilliseconds(200))
                    {
                        EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut },
                    });
            }
        }

        private void LoadGhost()
        {
            if (Preview.Data == null) { Preview.Ghost = null; return; }
            try
            {
                using (var d = new Desktop())
                {
                    // 只标注「会移动」的那些图标现在的位置，避免满屏虚线
                    var saved = new Dictionary<string, IconPos>(StringComparer.OrdinalIgnoreCase);
                    foreach (var it in Preview.Data.Icons) saved[it.Name] = it;

                    var moved = new List<IconPos>();
                    foreach (var cur in d.Snapshot())
                    {
                        if (!saved.TryGetValue(cur.Name, out var target)) continue;
                        if (target.X != cur.X || target.Y != cur.Y) moved.Add(cur);
                    }
                    Preview.Ghost = moved;
                }
            }
            catch
            {
                Preview.Ghost = null;
            }
        }

        private void Ghost_Click(object sender, RoutedEventArgs e)
        {
            if (Preview.Data == null) { ShowToast("先选中一个布局", "info"); return; }

            _showGhost = !_showGhost;
            if (_showGhost)
            {
                LoadGhost();
                if (Preview.Ghost == null) { _showGhost = false; ShowToast("读取当前桌面位置失败", "err"); return; }

                BtnGhost.Content = "隐藏当前位置";
                BtnGhost.Foreground = new SolidColorBrush(Color.FromRgb(0x9C, 0xB0, 0xFF));
                BtnGhost.BorderBrush = new SolidColorBrush(Color.FromRgb(0x3E, 0x4F, 0x86));
                BtnGhost.Background = new SolidColorBrush(Color.FromRgb(0x1B, 0x22, 0x36));

                ShowToast(Preview.Ghost.Count == 0
                    ? "这份布局和当前桌面完全一致，没有需要移动的图标"
                    : $"蓝色虚线是 {Preview.Ghost.Count} 个图标现在的位置",
                    Preview.Ghost.Count == 0 ? "ok" : "info");
            }
            else
            {
                Preview.Ghost = null;
                BtnGhost.Content = "叠加当前位置";
                BtnGhost.Foreground = new SolidColorBrush(Color.FromRgb(0x8A, 0x93, 0xA8));
                BtnGhost.BorderBrush = new SolidColorBrush(Color.FromRgb(0x2B, 0x31, 0x40));
                BtnGhost.Background = new SolidColorBrush(Color.FromRgb(0x17, 0x1B, 0x23));
            }
        }

        private LayoutEntry Selected => LayoutList.SelectedItem as LayoutEntry;

        private void UpdateButtons()
        {
            bool has = Selected != null;
            BtnRestore.IsEnabled = has;
            BtnDiff.IsEnabled = has;
            BtnDelete.IsEnabled = has;
            HintText.Text = has ? "双击列表项可直接恢复" : "先保存一份布局吧";
        }

        // ------------------------------------------------------------ 保存
        private async void Save_Click(object sender, RoutedEventArgs e)
        {
            int count = 0;
            try { using (var d = new Desktop()) count = d.Count; }
            catch (Exception ex) { ShowToast(ex.Message, "err"); return; }

            if (count == 0) { ShowToast("桌面上没有图标，无法保存", "err"); return; }

            string def = DateTime.Now.ToString("MM-dd HH:mm");
            bool ok = await AskAsync("保存当前布局",
                $"将记录桌面上 {count} 个图标的位置。给它起个名字：", def, "保存", true);
            if (!ok) return;

            string name = NameBox.Text.Trim();
            if (name.Length == 0) { ShowToast("名字不能为空", "err"); return; }

            try
            {
                using (var d = new Desktop())
                {
                    var layout = new LayoutFile
                    {
                        Name = name,
                        Created = DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss"),
                        Screen = new ScreenInfo
                        {
                            W = Native.GetSystemMetrics(0),
                            H = Native.GetSystemMetrics(1),
                        },
                        AutoArrange = d.AutoArrange,
                        Icons = d.Snapshot(),
                    };
                    Store.Save(layout);
                    ShowToast($"已保存「{name}」· {layout.Icons.Count} 个图标");
                }
                Refresh();
            }
            catch (Exception ex)
            {
                ShowToast(ex.Message, "err");
            }
        }

        // ------------------------------------------------------------ 恢复
        private void Restore_Click(object sender, RoutedEventArgs e) => DoRestore();

        private void LayoutList_DoubleClick(object sender, MouseButtonEventArgs e)
        {
            if (FindAncestor<ListBoxItem>(e.OriginalSource as DependencyObject) == null) return;
            DoRestore();
        }

        private void DoRestore()
        {
            var entry = Selected;
            if (entry == null) return;

            try
            {
                using (var d = new Desktop())
                {
                    var (n, missing, extra) = d.Restore(entry.Data);
                    string msg = $"已恢复 {n} 个图标到「{entry.DisplayName}」";
                    if (missing.Count > 0) msg += $" · {missing.Count} 个未找到";
                    ShowToast(msg, missing.Count > 0 ? "info" : "ok");
                    StatusText.Text = extra.Count > 0
                        ? $"还有 {extra.Count} 个图标不在该布局中，已保持原位"
                        : "已按布局还原桌面";
                }
                if (_showGhost) LoadGhost();
            }
            catch (Exception ex)
            {
                ShowToast(ex.Message, "err");
            }
        }

        // ------------------------------------------------------------ 预览差异
        private void Diff_Click(object sender, RoutedEventArgs e)
        {
            var entry = Selected;
            if (entry == null) return;
            try
            {
                using (var d = new Desktop())
                {
                    var (move, missing, extra, total) = d.Plan(entry.Data);
                    string msg = move == 0
                        ? $"「{entry.DisplayName}」与当前桌面完全一致"
                        : $"「{entry.DisplayName}」有 {move} 个图标会移动";
                    ShowToast(msg, move == 0 ? "ok" : "info");
                    StatusText.Text =
                        $"布局含 {total} 个图标 · 待移动 {move} · 已消失 {missing.Count} · 新增 {extra.Count}";
                }
            }
            catch (Exception ex)
            {
                ShowToast(ex.Message, "err");
            }
        }

        // ------------------------------------------------------------ 删除
        private async void Delete_Click(object sender, RoutedEventArgs e)
        {
            var entry = Selected;
            if (entry == null) return;
            bool ok = await AskAsync("删除布局",
                $"确定要删除「{entry.DisplayName}」吗？此操作不可撤销。", null, "删除", false);
            if (!ok) return;
            try
            {
                Store.Delete(entry);
                ShowToast($"已删除「{entry.DisplayName}」");
                Refresh();
            }
            catch (Exception ex) { ShowToast(ex.Message, "err"); }
        }

        // ------------------------------------------------------------ 对话框
        private Task<bool> AskAsync(string title, string desc, string input, string okText, bool withInput)
        {
            DlgTitle.Text = title;
            DlgDesc.Text = desc;
            DlgOk.Content = okText;
            _dialogHasInput = withInput;
            InputWrap.Visibility = withInput ? Visibility.Visible : Visibility.Collapsed;

            if (withInput)
            {
                NameBox.Text = input ?? "";
                NameBox.SelectAll();
            }

            Mask.Visibility = Visibility.Visible;
            _tcs = new TaskCompletionSource<bool>();
            if (withInput)
            {
                NameBox.Focus();
                Keyboard.Focus(NameBox);
            }
            return _tcs.Task;
        }

        private void CloseDialog(bool result)
        {
            Mask.Visibility = Visibility.Collapsed;
            var t = _tcs;
            _tcs = null;
            t?.TrySetResult(result);
        }

        private void DlgOk_Click(object sender, RoutedEventArgs e)
        {
            if (_dialogHasInput && NameBox.Text.Trim().Length == 0)
            {
                ShowToast("名字不能为空", "err");
                return;
            }
            CloseDialog(true);
        }

        private void DlgCancel_Click(object sender, RoutedEventArgs e) => CloseDialog(false);

        private void Mask_Click(object sender, MouseButtonEventArgs e)
        {
            if (e.OriginalSource == Mask) CloseDialog(false);
        }

        private void NameBox_KeyDown(object sender, KeyEventArgs e)
        {
            if (e.Key == Key.Enter) { DlgOk_Click(sender, e); e.Handled = true; }
            else if (e.Key == Key.Escape) { CloseDialog(false); e.Handled = true; }
        }

        // ------------------------------------------------------------ Toast
        private void ShowToast(string msg, string kind = "ok")
        {
            string color = kind == "err" ? "#F0616D" : kind == "info" ? "#5B7CFF" : "#3DDC97";
            ToastDot.Fill = new SolidColorBrush((Color)ColorConverter.ConvertFromString(color));
            ToastText.Text = msg;

            Toast.BeginAnimation(OpacityProperty,
                new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(150)));
            ToastShift.BeginAnimation(TranslateTransform.YProperty,
                new DoubleAnimation(14, 0, TimeSpan.FromMilliseconds(220))
                {
                    EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut },
                });

            _toastTimer.Stop();
            _toastTimer.Start();
        }

        // ------------------------------------------------------------ 杂项
        private void LayoutList_SelectionChanged(object sender, SelectionChangedEventArgs e)
        {
            UpdateButtons();
            ShowPreview();
        }

        private void TitleBar_MouseDown(object sender, MouseButtonEventArgs e)
        {
            if (e.ChangedButton != MouseButton.Left) return;
            try { DragMove(); } catch { }
        }

        private void Min_Click(object sender, RoutedEventArgs e) => WindowState = WindowState.Minimized;

        private void Close_Click(object sender, RoutedEventArgs e) => Close();

        private void Folder_Click(object sender, RoutedEventArgs e)
        {
            try { Process.Start(new ProcessStartInfo("explorer.exe", Store.Dir) { UseShellExecute = true }); }
            catch { }
        }

        private static T FindAncestor<T>(DependencyObject d) where T : DependencyObject
        {
            while (d != null)
            {
                if (d is T t) return t;
                d = VisualTreeHelper.GetParent(d);
            }
            return null;
        }
    }
}
