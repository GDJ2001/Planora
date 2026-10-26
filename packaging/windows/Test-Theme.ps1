$ErrorActionPreference = 'Stop'
$themePath = Join-Path $PSScriptRoot 'Planora-Theme.xml'
[xml]$theme = Get-Content -LiteralPath $themePath -Raw
$ns = New-Object System.Xml.XmlNamespaceManager($theme.NameTable)
$ns.AddNamespace('t', 'http://wixtoolset.org/schemas/v4/thmutil')

# WixStdBA owns state transitions. A hand-authored page outside its page set
# can remain visible when Burn advances to Progress or Success.
$supportedPages = @('Loading', 'Help', 'Install', 'Modify', 'Progress', 'Success', 'Failure')
$pages = @($theme.SelectNodes('//t:Page', $ns))
foreach ($page in $pages) {
    $pageName = $page.GetAttribute('Name')
    if ($pageName -notin $supportedPages) {
        throw "Unsupported WixStdBA page: $pageName"
    }
}
if ($pages.Count -ne $supportedPages.Count) {
    throw 'The installer theme must contain each required WixStdBA page exactly once.'
}
foreach ($name in $supportedPages) {
    if (@($pages | Where-Object { $_.GetAttribute('Name') -eq $name }).Count -ne 1) {
        throw "Missing or repeated WixStdBA page: $name"
    }
}

$uninstallButtons = @($theme.SelectNodes('//t:Button[@Name="UninstallButton"]', $ns))
if ($uninstallButtons.Count -ne 1 -or $uninstallButtons[0].ParentNode.GetAttribute('Name') -ne 'Modify') {
    throw 'The uninstall action must appear exactly once on the supported Modify page.'
}
if ($theme.SelectNodes('//t:ChangePageAction', $ns).Count -ne 0) {
    throw 'Do not navigate to a custom installer page outside WixStdBA state management.'
}

$names = @($theme.SelectNodes('//*[@Name]', $ns) | ForEach-Object { $_.GetAttribute('Name') })
$duplicates = @($names | Group-Object | Where-Object Count -gt 1)
if ($duplicates.Count -gt 0) {
    throw "Duplicate theme control names: $(($duplicates.Name) -join ', ')"
}

Write-Output 'Planora installer theme structure is valid.'
