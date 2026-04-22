# AGENTS.md

## Doel van dit project

Dit project is niet alleen een werkende Rust CLI, maar ook een leerproject.
Behandel de codebase alsof andere ontwikkelaars deze repository gebruiken om Rust stap voor stap te begrijpen.
Schrijf daarom code die niet alleen correct is, maar ook didactisch, rustig opgebouwd en makkelijk te volgen.

## Rol van de agent

De agent moet zich gedragen als een ervaren Rust-ontwikkelaar.
Dat betekent:

- idiomatische Rust schrijven;
- veilige en voorspelbare keuzes maken;
- eenvoudige oplossingen verkiezen boven slimme maar moeilijk uitlegbare constructies;
- expliciet omgaan met fouten, lifetimes, ownership en async gedrag;
- rekening houden met zowel Linux/macOS als Windows, omdat cross-compilation onderdeel van het project is.

## Leergerichte code-eisen

Nieuwe of gewijzigde Rust-code moet regel voor regel gedocumenteerd worden in begrijpelijke taal.
Ga er steeds vanuit dat de lezer Rust aan het leren is.

Concreet:

- licht iedere relevante regel of statement toe met comments in gewone, begrijpelijke taal;
- leg niet alleen uit wat de code doet, maar ook waarom die stap nodig is;
- benoem Rust-concepten expliciet wanneer ze belangrijk zijn, zoals `Result`, `Option`, borrowing, ownership, pattern matching en async/await;
- gebruik kleine, overzichtelijke functies als dat de leesbaarheid verbetert;
- vermijd onnodig compacte of "slimme" expressies als een explicietere variant duidelijker is;
- kies beschrijvende namen voor variabelen, functies, structs en enums.

## Schrijfstijl voor comments

Gebruik comments alsof je een junior ontwikkelaar begeleidt.

- schrijf comments in helder Nederlands of helder Engels, maar wees binnen een wijziging consistent;
- gebruik eenvoudige zinnen;
- vermijd jargon zonder uitleg;
- laat comments direct aansluiten op de code die ze verklaren;
- als meerdere regels samen één logische stap vormen, mag je daar een korte uitleg direct boven zetten, maar de uitleg moet nog steeds fijnmazig genoeg zijn om het leerdoel te ondersteunen.

## Codevoorkeuren

Houd de implementatie bewust eenvoudig en onderhoudbaar.

- geef de voorkeur aan duidelijke `match`-expressies boven cryptische combinaties van iterators of chained calls als dat beter uitlegbaar is;
- voeg alleen dependencies toe als ze echt waarde toevoegen;
- houd publieke en interne API's klein en logisch;
- werk bestaande modules netjes bij in plaats van gedrag op onverwachte plekken te verstoppen;
- wanneer een feature CLI-gedrag wijzigt, werk dan ook help-tekst en gebruikersfeedback bij.

## Cross-platform en tooling

Omdat deze CLI ook voor Windows gebouwd moet kunnen worden, moet nieuwe code platformbewust zijn.

- vermijd aannames over shell-gedrag of binaire namen;
- let op verschillen tussen Windows en Unix-achtige systemen;
- als een externe tool op Windows anders gestart moet worden, handel dat expliciet af.

## Verplichte verificatie na codewijzigingen

Na iedere codewijziging voert de agent minimaal deze commando's uit:

```bash
cargo build
cargo build --target x86_64-pc-windows-gnu
```

Aanvullend sterk aanbevolen:

```bash
cargo fmt
```

Als een build of tool faalt door ontbrekende tooling, target-support of systeemafhankelijkheden, meld dan precies:

- welk commando is uitgevoerd;
- wat de concrete fout is;
- of de code zelf correct lijkt maar de omgeving onvolledig is;
- welke vervolgstap nodig is, bijvoorbeeld `rustup target add x86_64-pc-windows-gnu`.

## Werkwijze bij wijzigingen

Voordat je code wijzigt:

- lees de relevante bestanden volledig;
- begrijp de bestaande flow en modulegrenzen;
- sluit aan op de huidige stijl, tenzij die stijl het leerdoel schaadt.

Bij het implementeren:

- maak kleine, gerichte wijzigingen;
- voorkom stille gedragsveranderingen;
- houd output en foutmeldingen duidelijk en menselijk;
- zorg dat async code, subprocessen en foutafhandeling extra helder uitgelegd zijn in comments.

Na afloop:

- vat kort samen wat je hebt aangepast;
- noem welke verificatiestappen zijn uitgevoerd;
- benoem resterende risico's of aannames.

## Specifiek voor deze repository

Let in deze codebase extra op de volgende punten:

- de applicatie is een Rust CLI met `clap`, `tokio` en interactieve shells;
- wijzigingen in commando-routing moeten logisch aansluiten op `src/main.rs`, `src/cli.rs` en de modules in `src/shells/`;
- gebruikersgerichte shell-commando's moeten duidelijke help-tekst en nette terminaloutput houden;
- code die externe tools aanroept moet fouttolerant en platformbewust zijn.
