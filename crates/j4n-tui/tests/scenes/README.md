# Scenes

A scene is a TOML list of what happens to the screen; `scene_goldens` (in `crates/j4n/tests/laws/`) runs each one at its sizes
and compares the screen with `../golden/<scene>[.<shot>].<cols>x<rows>.{txt,ansi}`. `cat` an `.ansi` file to see it in colour.

```toml
doc   = "what the scene shows"
sizes = [[160, 48], [80, 24]]          # default [[80, 24]]

[[step]]                               # a message from the core: the `Down` of j4n-proto, as a table
down = "hello"
api = 1
name = "j4n"
version = "0.0.0"
project = "demo"

[[step]]
key = "ctrl+k"                         # enter esc tab backtab space up down left right home end pageup pagedown f1..f12, a letter; ctrl+ alt+ shift+
[[step]]
text = "typed letters"                 # \n is Enter
[[step]]
paste = "one piece of text"
[[step]]
resize = [100, 30]
[[step]]
wait = 800                             # milliseconds on the scene's own clock
[[step]]
shot = "after-the-ask"                 # a picture here, named <scene>.<shot>; the end of the scene is always one
```

After a change to what is drawn, `J4N_BLESS=1 scripts/t test -p j4n --test laws scene_goldens` writes the goldens; read the diff of the
golden files before committing them. Every golden is also checked for spacing: an icon is followed by a blank cell.
