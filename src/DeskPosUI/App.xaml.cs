using System.IO;
using System.Windows;

namespace DeskPos
{
    public partial class App : Application
    {
        protected override void OnStartup(StartupEventArgs e)
        {
            base.OnStartup(e);
            DispatcherUnhandledException += (s, args) =>
            {
                try
                {
                    string log = Path.Combine(Path.GetTempPath(), "deskpos_error.log");
                    File.AppendAllText(log, DateTime.Now + " " + args.Exception + Environment.NewLine);
                }
                catch { }

                MessageBox.Show(args.Exception.Message, "DeskPos 出错了",
                    MessageBoxButton.OK, MessageBoxImage.Warning);
                args.Handled = true;
            };
        }
    }
}
