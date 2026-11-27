# Liquid Glass für Quickshell und Hyprland

> **Neu hier? Einfach ein UI, Overlay oder eine eigene App (z. B. Icon Composer) bauen?**
> Siehe **[QUICKSTART.md](QUICKSTART.md)** für die kompakte API-Referenz (Hyprland-IPC, QML-Wrapper, Rollen-Tabelle, Dual-Tint, Mitosis sowie C-/Rust-ABI für eigenständige Apps).

Diese Einbindung rendert Liquid Glass über dem Bildbereich hinter einer einzelnen Quickshell-Fläche. Rust packt Material und Geometrie. Die GPU verarbeitet den Ausschnitt, das Distanzfeld, Blur und Komposition direkt im Hyprland-Kontext. Hyprland zeichnet den Widget-Inhalt anschließend regulär. Das Plugin hat einen eigenen Namen und eigene Layer-Namespaces und läuft mit deaktiviertem Hyprglass.

Das Plugin ist für die installierten Hyprland-0.57.2-Header gebaut. Es prüft beim Laden den vollständigen ABI-Hash. Der Quickshell-Patch schaltet rechteckige Widgets im bestehenden Compositor-Modus auf den GPU-Pass um. Material, Hell/Dunkel, Radius und Diffusion stammen aus dem Widget. Alpha-geformte Widgets wie die Glasuhr behalten ihren bisherigen Zeichenpfad. Gedrehte Ausgänge und HDR fallen aus dem unterstützten SDR-Pfad heraus.

```sh
make -C integrations/hyprland
make -C integrations/hyprland check
```

Der isolierte Test rendert Regular und Clear in einem gepolsterten Zuschnitt eines 340×160-Punkte-Widgets bei 1,6× und vergleicht gegen den separaten Rust-Referenzrenderer. Auf Intel LNL und llvmpipe beträgt die Abweichung höchstens eine RGB8-Stufe. Ein asymmetrisches Muster bestätigt die Zeilenposition beim Lesen und Schreiben. Der GPU-Pass auf Intel LNL benötigte im Mittel 1,39 ms je warm gerendertem Regular-Widget über 20 Wiederholungen mit GPU-Synchronisierung am Ende der Serie.

Der frühere Desktop-Test mit synchronem Bildtransfer zwischen GPU und CPU zeigte falsch positionierte Glasflächen und spürbare Verzögerungen. Der neue Kopierpfad verwendet die monitorlokalen Zeilen in beiden Richtungen. Der GPU-Pass verarbeitet alle Zwischenbilder als Texturen.

Die aktive Session bei 1,6× registriert fünf Widget-Flächen. Die Prüfung im Desktop-Ausschnitt zeigte das Glas hinter den Batteriesymbolen. Der Widget-Launcher lädt die installierte Version aus `~/.local/share/liquidglass/liquid-glass-hyprland.so`, falls das Plugin noch fehlt. Ein Neustart der Widget-Shell mit zuvor entladenem Plugin bestätigte diesen Ladevorgang sowie die Bildproduktion mit null gemeldeten Passfehlern. Die Zähler sind über `hyprctl liquid-glass-status` abrufbar.

## Materialsteuerung ab Plugin 0.3

`quickshell-material-settings.patch` ergänzt den Widget-Launcher- und Oberflächenpatch. Die neuen Rust-Einstellungen werden unabhängig vom älteren Materialsystem gespeichert. Regular bei 50 % Diffusion und ohne zusätzliche Farbe ist der neue Standard für die rechteckigen Widgets. Clear ist ausdrücklich auswählbar. Das vorhandene Hell/Dunkel und der Eckradius dienen als Ausgangspunkt. Die globalen Rust-Regler überschreiben lokale Diffusion bei aktivem Alle-Schalter und lokale Farbe bei globaler Farbstärke über null.

Die Widgetoberfläche behält beim Ändern der Einstellungen denselben Wayland-Namespace. Quickshell übermittelt die Werte direkt an den Compositor. Rust verarbeitet Glasvariante, Diffusion und die sRGB-Farbe samt Stärke im Materialrezept, und die GPU rendert mit den daraus gebauten Uniforms. Die Regler sind in der aktiven Sitzung pro Widget und global geprüft. Der ausgelieferte Kalender übernimmt Regular bei 50 %, und sämtliche vorübergehend gesetzten Prüfwerte wurden aus der Benutzerkonfiguration entfernt. Tint-Bildtests mit Regular und Clear weichen auf Intel und llvmpipe höchstens einen RGB8-Wert vom Rust-Referenzrenderer ab. Der visuelle Vergleich mit macOS steht aus.

## Widget-Material und Legibility ab Plugin 0.4

`quickshell-legibility.patch` baut auf den beiden Quickshell-Patches auf. Rechteckige Rust-Widgets erhalten jetzt ein eigenes Widget-Material. Die Grundfarbe wird aus den vorhandenen Messungen an iOS-Home-Screen-Widgets unter drei bekannten Hintergründen und in beiden Erscheinungsmodi bestimmt. Die Referenz liegt in `~/projects/hyprglass-material/MEASUREMENTS.md` und den Beobachtungen unter `widget.observations` in dessen `calibration.json`. Die Messwerte beschreiben die Fläche. Brechung, Ränder und Geometrie verwenden weiter den vorhandenen rekonstruierten Glaspfad.

Der Regler **Liquid Glass · Lesbarkeit** wird getrennt von der älteren Diffusionsvorgabe gespeichert. Er steuert die Hintergrundunschärfe auf Basis der gemessenen Widget-Endpunkte von etwa 1,05 bis 15,5 Punkten. Die Umsetzung approximiert die Unschärfe über die Varianz der AGX2-Mipstufen. Die Platte ändert ihren Farbtransfer dabei nicht. Die optionale sRGB-Tönung wird getrennt als Einfärbung im linearen Farbraum angewendet. Der Launcher- und Rendererstandard ist 50 % Legibility ohne zusätzliche Tönung. Der belegte Default 0,5 für den macOS-Regler stammt aus dem [Dock-Pfad](../../demos/live-demo/DOCK.md). Es liegt kein eigener macOS-Widget-Default aus einem Original-Readback vor.

Auf Intel LNL und llvmpipe stimmen die sechs gemessenen Zielfarben in der Widget-Mitte auf eine RGB8-Stufe. Zwischen den Legibility-Endpunkten bleiben diese Farben bis auf eine Stufe gleich. Ein strukturiertes Raster bestätigt die zunehmende Unschärfe, der Farbtest die separate Einfärbung. Das Paket wurde auf dem Desktop installiert. Live-Änderungen pro Widget und global erreichten den selben Kalender-Layer ohne Neustart. Der vorherige globale Zusatz-Tint wurde für den Standardlook auf 0 % zurückgesetzt und die gespeicherte Farbauswahl beibehalten.

## Dock und Editor-Panels ab Plugin 0.5

Das Dock verwendet `DockRecipe.Other` und den aus Build 26B123 geprüften kachelgrößenabhängigen Radius. Die Quickshell-Schale meldet den pillenförmigen Inhalt getrennt von ihrer bildschirmbreiten Wayland-Fläche. Sie übergibt die aktuelle Größe und Verschiebung aus ihrer laufenden Animation. Das Dock nutzt die gespeicherte Dock-Lesbarkeit oder den explizit gemeinsam aktivierten Rust-Regler. Hover, Magnification und Ausblenden bleiben in der bestehenden Dock-Oberfläche.

Für Widget-Einstellungen und Editor-Palette wurde `CartouchePopover` im Originalcode aus Build 26A434 in 24 aktiven SDR-Konfigurationen ausgeführt. Darunter sind vier Größen, Hell/Dunkel und drei Reglerpositionen. Der Panel-Port übernimmt unter anderem 5 pt innere Brechungshöhe, die höhenabhängige Außenbrechung, den im untersuchten Kontext konstanten Blur-Radius 10 und die Füllstufe mit Radius 16. Die Vorschau startet aus Gestaltungsgründen bei 75 % Lesbarkeit. Der Wert ist im Look-Editor einstellbar. `CartouchePopover` ist das verwendete Apple-Rezept für diesen Port. Eine Zuordnung zu jeder nativen Systemeinstellungs-Oberfläche wurde nicht geprüft. [Extrahierte Parameter](popover-original-26A434.json)

Die beiden Panels wurden in der aktiven Sitzung mit dem Popover-Profil registriert. Eine vorübergehende Änderung an Dock-Kachelgröße und Randabstand änderte die gemeldete Dock-Geometrie von etwa 1180×71 auf 685×40 Punkte und ergab 209 gerenderte Dock-Pässe ohne gemeldeten Fehler. Die ursprünglichen Dock-Einstellungen wurden anschließend wiederhergestellt. Ein Bild- und Zeitvergleich von Dock- und Panel-Animationen mit macOS steht aus.
