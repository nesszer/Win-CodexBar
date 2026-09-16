param([string]$ImgPath)

Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Foundation, ContentType = WindowsRuntime]
$null = [Windows.Storage.StorageFile, Windows.Foundation, ContentType = WindowsRuntime]
$null = [Windows.Storage.Streams.IRandomAccessStream, Windows.Foundation, ContentType = WindowsRuntime]

function Await($WinRtTask, $ResultType) {
  $asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() |
    Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
  $asTask = $asTaskGeneric.MakeGenericMethod($ResultType)
  $netTask = $asTask.Invoke($null, @($WinRtTask))
  $netTask.Wait(-1) | Out-Null
  $netTask.Result
}

$file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($ImgPath)) ([Windows.Storage.StorageFile])
$stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$fmt = $decoder.BitmapPixelFormat
$alpha = $decoder.BitmapAlphaMode
$bmp = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
Write-Output "BITMAP: $fmt / $alpha / $($bmp.PixelWidth)x$($bmp.PixelHeight)"
$ocr = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
Write-Output "LANGS: $($ocr.RecognizerLanguage.LanguageTag)"
$res = Await ($ocr.RecognizeAsync($bmp)) ([Windows.Media.Ocr.OcrResult])
foreach ($line in $res.Lines) { Write-Output ("LINE: " + $line.Text) }
