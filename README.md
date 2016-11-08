# i3 tmux integration

This program aims to duplicate the iTerm "Tmux integration" on Linux, using i3
as the window manager.

## Tmux Notes

When starting tmux in "Command Mode" (see man), tmux will first output a DEC
sequence. We need to detect that sequence to figure out when command mode is
started. The sequence is `[0o33, 'P', '1', '0', '0', '0', 'p']`

Once this sequence is detected, the stdin/stdout of that terminal will be your
primary means of communication with tmux. On stdin, tmux will send you events
that you'll need to know about to draw correctly.

On stdout, you can send tmux commands. Those are the "usual" commands that you
would usually type with Ctrl+B :.

Some useful commands :

- `send-keys -t <pane> <keys>`: Send the keys as a series of hex sequences so
tmux understands that they are key sequences.



To tell you about how to show the windows, tmux has a weird layout format. The
"inner" format looks like this :
`<width>x<height>,<offx>,<offy>(,<panelid>|{<inner layout>}|[<inner layout>]),<inner layout>`

width, height, offx and offy are all in "character count".

If an inner layout is inside `{}` brackets, it is a left/right layout. If it is
in `[]` brackets, it is a top/down layout.

Every window has a root layout that looks like `[layout <hash>,<inner layout>]`.
This is what will be sent by tmux in the `layout-change` event. A useful link
for layout-change : https://github.com/tmux/tmux/blob/d54e990c4ffb576aa3d82306b970dc64bdd4cda6/control-notify.c#L7jjj
