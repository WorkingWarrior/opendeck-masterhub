# Now Playing — raport implementacji

## Architektura

Akcja `io.github.opendeck-masterhub.nowplaying` używa jednego asynchronicznego monitora
`zbus` na sesyjnym D-Bus dla wszystkich widocznych instancji. Po ostatnim
`will_disappear` zamyka połączenie i czeka na nową instancję bez odpytywania.
Nazwy MPRIS są sortowane; tryb Auto preferuje grającą usługę, potem wstrzymaną,
a następnie pozostałe. W każdej grupie wygrywa pierwsza nazwa w porządku leksykalnym.
Wskazana usługa jest wybierana dokładnie po nazwie D-Bus. Sygnały
`NameOwnerChanged`, `PropertiesChanged` i `Seeked` aktualizują stan. Po zmianie
właściwości odpytywany jest tylko jej nadawca. Pozycja pomiędzy sygnałami jest
interpolowana lokalnie z uwzględnieniem `Rate` i ograniczona czasem utworu.
Wywołania D-Bus mają limit 3 s, a ponowne połączenia odstęp 5 s. Kolejka sterowania
ma limit 64 zdarzeń; przepełnienie jest logowane. Zapis głośności aktualizuje lokalną
bazę kolejnego obrotu dopiero po sukcesie odpowiedzi D-Bus.

Metadane obejmują tytuł, wykonawcę, album, adres okładki i czas trwania. Czas jest
odczytywany zarówno z `i64` zgodnego z MPRIS, jak i `u64` stosowanego przez Spotify;
wartości spoza zakresu `i64` są odrzucane. Długość ze Spotify może dzięki temu
być użyta do rysowania postępu. Brak pozycji lub długości ukrywa pierścień zamiast
rysować zmyślony postęp. Odczytywane są też stan, pozycja, głośność i możliwości `CanControl`, `CanPlay`, `CanPause`,
`CanSeek`. Brak opcjonalnej właściwości nie zatrzymuje akcji. `PlayPause`, `Seek`
(mikrosekundy MPRIS) i zapis `Volume` są wysyłane tylko, gdy właściwe możliwości
i właściwości są dostępne. Nieudany zapis jest logowany, bez udawania sukcesu.

Renderowanie rozszerza istniejący `src/render.rs`. Dial dostaje obraz RGB 240×240
z okładką wypełniającą cały ekran (centralne przycięcie bez rozciągania), dolnym
gradientem pod napisami, pierścieniem postępu i oznaczeniem pauzy wewnątrz
bezpiecznego koła. Wykonawca jest nad tytułem; długie napisy są skracane wielokropkiem.
Ustawienie „Kolor akcentu” (`accentColor`, domyślnie `#a0dc30`) zmienia pierścień,
napis wykonawcy i ikonę nuty. Kolor jest zapisywany osobno dla każdej instancji akcji;
brak ustawienia lub niepoprawny zapis koloru daje domyślny limonkowy.
Pole „Czcionka” (`fontFamily`, domyślnie `auto`) wybiera rodzinę fontu dla napisów
tej instancji Now Playing. Lista obejmuje dostępne fonty systemowe i użytkownika;
jest buforowana do ponownego uruchomienia pluginu. Pliki TTF/OTF oraz kolekcje TTC
są odczytywane z właściwym indeksem kroju. Nieobecny font przywraca krój domyślny.
Brakujące znaki są rysowane fontem zastępczym CJK (preferowany Noto Sans CJK JP),
również w tekstach mieszających alfabet łaciński i japoński. Pomiar i skracanie
używają tych samych fontów co rysowanie. Fonty są wczytywane poza wątkami async
i buforowane; plugin nie dołącza ich plików. Na Linuksie potrzebny jest dostępny
font japoński, np. pakiet `noto-fonts-cjk` na Arch/CachyOS. W Flatpaku lista zależy
od fontów udostępnionych w piaskownicy (uwzględniane są `/run/host/fonts` i
`/run/host/user-fonts`). Nowe fonty wymagają ponownego uruchomienia pluginu.
Klawisz 15xKey dostaje obraz 72×72; na tak małym ekranie
pokazuje okładkę albo ikonę nuty, bez dodatkowych napisów. Przy braku odtwarzacza
Dial pokazuje ikonę i tekst „Brak odtwarzacza”, a klawisz ikonę zastępczą; sterowanie
nic nie wysyła. Okładki z `file://` i HTTP(S) są pobierane poza wątkami async,
z limitem 4 MiB danych, 4000 px wymiaru, 32 MiB alokacji dekodera i 3 s dla HTTP.
Jeden worker pobiera okładki, a drugi rysuje najnowszą migawkę odtwarzaczy i wysyła
obrazy. HTTP, dekodowanie ani HID nie zatrzymują pętli odbierającej sygnały MPRIS.
Podczas pobierania wyświetlana jest ikona nuty. Bufor LRU zawiera do 12 okładek
240×240, również zapisy nieudanych pobrań; ponowienie następuje najwcześniej po 30 s.
Zadania pobierania są współdzielone i nie są powielane przez instancje akcji.

Dial korzysta z istniejącego `overlay` i blokady przejścia GIF. Właściciel ekranu
jest przypisany do kontekstu akcji; spóźniona klatka poprzedniej akcji nie może
nadpisać nowej. Nakładka głośności ma pierwszeństwo i po zakończeniu przywraca
ostatni obraz Now Playing pod tą samą blokadą przejścia. Zmiana ustawień unieważnia
starą klatkę. Nieudany zapis HID nie oznacza obrazu jako wysłanego, więc można go
ponowić. Przejęcie ekranu przez widget zatrzymuje poprzedni GIF. Po zatrzymaniu GIF-a aktualny obraz bezpośredni jest
ponownie wysyłany. Identyczne obrazy Dial i klawiszy są pomijane; cykliczne
i sygnałowe renderowanie jest ograniczone do jednej klatki na sekundę.

## Pliki

- Dodane: `src/nowplaying.rs`, `pi/nowplaying.html`, ten raport.
- Zmienione: `src/main.rs`, `src/actions.rs`, `src/render.rs`, `src/overlay.rs`,
  `manifest.json`, `pl.json`, `Cargo.toml`, `Cargo.lock`, `README.md`.
- `package.py` pozostał bez zmian: automatycznie zbiera pliki z `pi/` i wpisy
  manifestu. Ikona listy akcji używa istniejącego zasobu `icons/volume`.
- Zastane pliki użytkownika `clock-check.png` i `docs/plan_naprawy_wayland.md`
  pozostały nietknięte.

## Przypisanie

Na MasterHUB 1xDial należy przypisać Now Playing **osobno** do Keypad
(ekran i naciśnięcie) oraz Encoder (obrót i ewentualne naciśnięcie enkodera).
Jedno przypisanie nie tworzy drugiego. Na 15xKey przypisuje się akcję do
wybranego klawisza. Panel pozwala wybrać Auto lub usługę, tryb obrotu,
krok przewijania, widoczność okładki, postępu i wykonawcy, kolor akcentu i czcionkę.

## Walidacja i ograniczenia

- `cargo check` — PASS.
- `cargo test` — PASS, 10 istniejących testów; nie dodano ani nie zmieniono testów.
- `cargo build --release` — PASS dla Linuksa.
- `CARGO_TARGET_DIR=.../target sh build-linux.sh` i `python3 package.py` — PASS;
  paczki EN/PL zawierają aktualny plik Linuksa, panel i poprawne ścieżki zasobów.
- `cargo clippy --all-targets --locked` — kod wyjścia 0. Pozostały zastane uwagi
  w innych akcjach i przykładach; tryb `-D warnings` nie przechodzi całego
  repozytorium. Nie wyłączono lintów ani nie refaktoryzowano pozostałych akcji.
- `rustfmt --check` dla `nowplaying.rs`, `overlay.rs` i `render.rs` — PASS;
  nie formatowano całego repozytorium.
- `git diff --check` — PASS.
- Skrypt panelu `pi/nowplaying.html` sprawdzony przez `node --check` — PASS.
- Jednorazowy odczyt natywnym `zbus` z sesyjnego D-Bus potwierdził dekodowanie
  `GetAll`, zagnieżdżonego `Metadata`, tytułu, tablicy wykonawców, długości i stanu
  z działającej usługi `org.mpris.MediaPlayer2.plasma-browser-integration`.
  To potwierdza protokół, **nie** uruchomienie akcji w OpenDeck ani działanie HID.
- Dostępny jest tylko target `x86_64-unknown-linux-gnu`; kompilacji Windows/macOS
  nie zweryfikowano. Backend tych platform jawnie nie obsługuje MPRIS.
- Nie wykonano pełnego testu sprzętowego cyklu życia ani testu w OpenDeck Flatpak.
  Podglądy renderu potwierdziły tekst japoński, zmianę fontu i koloru oraz układ
  koła, także na jasnym tle i przy długich napisach. Odczyt z uruchomionego Spotify
  potwierdził przyczynę braku postępu (`u64`) i poprawne dekodowanie długości.
- Przed pakowaniem uruchom `sh build-linux.sh`, następnie `python3 package.py`
  (lub `--lang pl` / `--lang en`). Paczka bierze plik z `dist/bin`, nie bezpośrednio
  z `target/release`. Kopiowanie i pakowanie nie zmieniają aplikacji OpenDeck.
- KDE Plasma/Wayland nie wymaga Enigo dla tej akcji; odtwarzacz musi publikować
  MPRIS na tej samej sesji D-Bus. Flatpak może filtrować inne nazwy MPRIS.
  Sprawdź uprawnienia OpenDeck; w razie potrzeby nadaj wąskie
  `--talk-name=org.mpris.MediaPlayer2.*`. Lokalne `file://` poza sandboxem i HTTP(S)
  bez dostępu do sieci dają ikonę zastępczą. Zobacz
  [dokumentację Flatpaka](https://docs.flatpak.org/en/latest/sandbox-permissions.html)
  i [MPRIS](https://specifications.freedesktop.org/mpris/latest/).

## Ręczne sprawdzenie

1. Zbuduj i zapakuj aktualną wersję, zainstaluj paczkę przez OpenDeck i uruchom
   go ponownie.
2. Przypisz akcję do Keypad 1xDial. Uruchom odtwarzacz MPRIS. Sprawdź okładkę,
   tytuł, wykonawcę i postęp; wstrzymaj oraz wznow odtwarzanie naciśnięciem.
3. Przypisz osobną instancję do Encoder 1xDial. Sprawdź przewijanie z ustawionym
   krokiem i tryb regulacji głośności. Sprawdź zachowanie odtwarzacza, który
   nie obsługuje `CanSeek` lub `Volume`.
4. Przypisz akcję do 15xKey i sprawdź miniaturę oraz naciśnięcie. Zmień utwór,
   przełącz Auto na wskazaną usługę i zamknij odtwarzacz.
5. Przełącz profil oraz inną akcję na ekranie Dial. Pokaż nakładkę głośności
   i sprawdź powrót obrazu. Zamień akcję GIF na Now Playing; stary GIF ani
   stara okładka nie powinny wracać. Sprawdź `masterhub.log` przy błędach.
6. Wybierz inny kolor i font. Zmień profil i wróć; ustawienia powinny pozostać.
   Odtwórz utwór z japońskim tytułem i sprawdź także font bez japońskich glifów,
   aby potwierdzić działanie fontu zastępczego na ekranie.
