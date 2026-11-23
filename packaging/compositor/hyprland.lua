-- Nova's Hyprland rules for a Lua config (all of 0.57), written by
-- `nova setup` to ~/.config/nova/hyprland.lua (or loaded by the home-manager
-- module) and required from Hyprland's hyprland.lua. hyprland.conf beside
-- this file says the same in the old format.

hl.config({
    misc = {
        -- Hyprland refuses a new lock screen after the one holding the lock
        -- stopped, so a lock screen that crashes or restarts leaves the
        -- session locked with nothing to type into. With this, novad's
        -- restarted UI locks again.
        allow_session_lock_restore = true,
    },
})
