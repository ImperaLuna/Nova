//@ pragma AppId nova-files-picker

import "files"

// Apps' Open and Save dialogs, as NovaFilesPicker: Nova Files windows in
// picker mode, under an app id of their own so compositors float them over
// the app (packaging/compositor).
FilesShell {
    pickers: true
}
