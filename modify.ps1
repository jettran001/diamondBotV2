Script to modify file
$content = Get-Content -Path "snipebot/src/snipebot.rs"; $content[3134..3144] = $null; $content | Set-Content -Path "snipebot/src/snipebot.rs"
