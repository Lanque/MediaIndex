# MediaIndex: analüüsi kiiruse, kuluhinnangu ja edasise arenduse plaan

Kuupäev: 2026-09-05. Staatus: koodi ja dokumentatsiooni ülevaatusel põhinev ettepanek; rakenduse käitumist ei ole muudetud.

> Ajalooline lähteplaan: allolevad leiud kirjeldavad 2026-09-05 seisu. Hilisemad parandused ja testitõendid on [agendi töölogis](agent-work-log.md) ning [release'i kontrollis](release-verification.md).

## 1. Soovitus

Järgmine arendusetapp peaks muutma kohaliku AI-analüüsi ennustatavaks: enne alustamist näeb kasutaja hinda, ajavahemikku ja analüüsi katvust; töö ajal säilivad tulemused ning eelarve piirab järgmisi päringuid. Alles seejärel tasub suurendada paralleelsust ja laiendada pilvetaristut.

Praegune projekt ei alusta nullist. Olemas on lokaalne indeks, kordusanalüüsi vältimine aktiivse mudeli järgi, duplikaatide koondamine, kaadrite/päringute eelhinnang, esialgne ja kalibreeritud ajahinnang, peatamine, kahe pilvefaili paralleeltöö ning mitme mudeli tulemuste säilitamine. Neid osi tuleks edasi arendada, mitte uuesti ehitada.

**Peamine lünk:** praegune „Checking cost…” tegevus arvutab töömahtu, mitte raha. Mudelite kataloogis on hinnakirjatekste, kuid need ei ole käivitatava töö kulukalkulaator.

## 2. Ülevaatuse piirid

- Läbi vaadati `docs` põhikirjeldused ning desktopi planeerimise, skannimise, AI-päringute, salvestamise, otsingu ja kasutajaliidese teostus.
- Tegemist on staatilise ülevaatusega. Kasutaja aeglast kausta, paigaldatud rakenduse versiooni, teenusepakkuja arvet ega päriselulisi etapiajastusi ei mõõdetud. Seetõttu on aegluse põhjused allpool koodist tuletatud kandidaadid, mitte mõõdetud protsentuaalne jaotus.
- Tasulisi päringuid, installimist, GitHubi muudatusi ega AWS-i tegevusi ei tehtud. Kehtivaid mudelihindu ja saadavust ei kontrollitud; olemasolevaid kataloogihindu ei käsitleta kinnitatud hinnakirjana.
- Varasemad testitulemused on failis `release-verification.md` 2026-09-03 seisuga. Neid ei esitata selle ülevaatuse käigus uuesti läbitud testidena.

## 3. Konkreetsed leiud

Prioriteet P0 tähendab andmete või kulu usaldusväärsuse probleemi; P1 järgmise tootearenduse põhifunktsiooni; P2 hilisemat täiustust.

| Prioriteet | Leid ja tõendus koodis | Mõju / vajalik muudatus |
| --- | --- | --- |
| P1 | `src-tauri/src/lib.rs:59,219` annab kaadrid, vision-päringud ja audiokestuse, kuid mitte rahasummat. `src/main.ts:1593,1637` nimetab seda kulu kontrolliks. | Kasutaja ei saa otsustada, kas kausta analüüs on taskukohane. Lisada hinnavahemik ja eelarve enne käivitamist. |
| P0 | `lib.rs:354–429`: `thread::scope` ootab töötajate lõppu; alles seejärel loetakse tulemuste kanal ja salvestatakse SQLite’i. | Rakenduse krahh pika töö keskel võib kaotada ka juba valmis klippide tulemused. Edenemine võib nimetada klippe salvestatuks enne andmebaasikirjutust. Tarbida kanalit ja kinnitada tulemusi töö ajal. |
| P0 | `ai.rs:410–450,570`: vigased kaadripakid kogutakse vigade loendisse, kuid kui mõni pakk õnnestub, võib tagastuda edukas osaline tulemus ilma nende vigadeta. `lib.rs:479` kontrollib üksnes annotatsioonide olemasolu. | Osaline klipp võidakse järgmisel käivitusel vahele jätta. Vajalikud katvus, osalise töö olek ja ainult puuduvate pakkide jätkamine. |
| P0 | `scanner.rs:349–390`: üle 16 MiB failidel kasutatakse suurust ja kolme 256 KiB lõiku; `docs/local-index.md` kirjeldab kogu faili SHA-256 identiteeti. | Sama suuruse ja samade proovialadega eri failid võivad saada sama identiteedi. See pole SHA-256 krüptograafilise kollisiooni küsimus, vaid lugemata sisu. Vale duplikaat võib pärida teise video AI-tulemused või jääda analüüsimata. |
| P1 | `ai.rs:899–959`: fikseeritud intervalliga kaadrid võetakse video algusest kuni piirini. Vaikimisi OpenAI UI: 5 s ja 60 kaadrit. | Pika klipi puhul kaetakse ligikaudu esimesed 5 minutit, mitte kogu klipp. Näidata selgelt analüüsitud ajavahemikku ja pakkuda kogu video ülevaaterežiimi. Täpne kaadriarv sõltub FFmpegi ajastusest. |
| P1 | `ai.rs:381–508`: väljavõte, vision, heli ja embeddingud tehakse ühe faili sees järjest; kõik selle faili JPEG-id loetakse mällu. | Võimalikud dekodeerimise, võrgu ja mälu pudelikaelad. Mõõta etappe ja eraldada piiratud tööjärjekorrad. |
| P1 | `ai.rs:1356`: Gemini embeddingud saadetakse järjest ükshaaval. | 60 kaadrit võib tähendada umbes 8 vision-päringut ja kuni 60 embeddingupäringut; praegune dialoog ei näita kogu päringumahtu. Kontrollida pakktöötluse võimalust ja rakendada teenuse piirangutega sobiv lahendus. |
| P1 | `ai.rs:854`: kuni 4 katset, 180 s päringu timeout; retry-tsükkel ei kontrolli katkestamist. FFmpegi `.output()` ootab protsessi lõppu. | „Stop” võib viibida mitu minutit; korduskatse võib saata uue päringu ka pärast stoppi. Katkestamine peab kehtima ka retry ja kohaliku protsessi tasemel. |
| P1 | `ai.rs:1199,1344` parsib teenuse vastust, kuid kasutusstatistikat ei talletata. | Puudub tokenite, päringukatsete ja tegeliku hinnangulise kulu ajalugu; eelhinnangut ei saa võrrelda tulemusega. |
| P1 | `main.ts:424–488` kalibreerib kogu tööaja ühe vision-päringu suhtarvuks. | Failide pikkus, dekodeerimine, audiomaht, embeddingud ja paralleelsuse mõju segunevad. Teistsuguse kausta ETA võib olla eksitav. |
| P1 | `ai.rs:298` mudeli nimeruum ei sisalda kaadristamise seadeid, kontekstivihjet ega prompti versiooni. | Intervalli või analüüsi juhise muutus ei tee olemasolevat analüüsi automaatselt eristatavaks. Embeddingumudeli muutmine seob praegu kasutaja kogu analüüsi kordamisega. |
| P2 | `local_index.rs:594` loeb mudeli annotatsioonid ja JSON-vektorid, kaustafilter rakendub hiljem Rustis. | Suure arhiivi korral kasvavad otsingu töö ja mälukulu ka aktiivsest kaustast väljapoole. Mõõta ja kitsendada päringut enne vektorite lugemist. |

Täiendav vaikeseadete lahknevus: UI OpenAI piir on 60 kaadrit, Rusti puuduvate seadete fallback 120. Ühine normaliseeritud konfiguratsioon peab olema nii hinnangu kui täitmise alus. Rusti poolel tuleb kontrollida ka nulli, ebamõistlikult suuri arve ja vigaseid väärtusi, mitte loota ainult UI sisenditele.

## 4. Millest kulu hinnata?

### 4.1 Kaks kohalikku eelhinnangu etappi

1. **Kiire kausta ülevaade:** failide arv, kogumaht, toetatud tüübid ja juba teadaolev metadata. Kuvada kohe, et hinnang täpsustub; esialgu teadmata duplikaate ei tohi lugeda kindlalt tasuta vahelejäetavaks.
2. **Analüüsi eelkontroll:** kestused, audio olemasolu, kinnitatud unikaalsed failid, olemasoleva analüüsi katvus ja kasutaja valitud profiil. See annab enne AI-käivitust arvutatava tööplaani. Puuduv metadata tähendab nähtavat ebakindlust või konservatiivset ülempiiri, mitte nullkulu.

Faili GB-maht on kasulik kohaliku lugemise ja dekodeerimise ajahinnangus. AI-arve jaoks on olulisemad **analüüsitavate kaadrite arv ja mõõtmed, mudel, sisend-/väljundtokenid, audiominutid ja embeddinguteksti maht**. Sama kestusega erineva bitikiirusega videod võivad vajada sarnast AI-kulu, kuigi nende failisuurused erinevad mitu korda.

### 4.2 Üks ühine tööplaan

Planeerija peab genereerima konkreetse failide, ajatemplite, kaadripakkide ja helilõikude loendi; käivitaja täidab just selle plaani. Planeerimise ja FFmpegi eraldi ligikaudsed algoritmid võivad praegu lahkneda, eriti väga lühikeste videote puhul.

Praeguse algusosa režiimi lihtsustatud mahuvalem:

```text
F_i ≈ min(ceil(video_kestus_i / intervall), kaadripiir)
V_i = ceil(F_i / vision_paki_suurus)
A_i = min(video_kestus_i, intervall × kaadripiir), kui heli on olemas ja valitud
```

Uues lahenduses tuleneb täpne plaanitud `F_i` ajatemplite loendist. Hinnang peab summeerima pakid failikaupa; kogu kaadrisumma jagamine kaheksaga alahindab väikeste failide päringuid.

```text
kulu = vision_sisendtokenid × sisendi_tariif
     + vision_väljundtokenid × väljundi_tariif
     + embedding_tokenid × embeddingu_tariif
     + audio_arveldusühikud × audio_tariif
     + teadaolevad muud teenuse tasud
```

Tokenimäärad normaliseerida tariifi ühikule, näiteks miljonile tokenile. Sisend hõlmab pilte, prompti ja skeemi kordust. Arvestada mudelipõhist pildiarvestust, väljundipiiri ning võimalikke eraldi arveldusreegleid. Üks üldine „hind kaadri kohta” sobib ainult selgelt märgitud kalibreeritud ligikaudseks hinnanguks.

Näidata madalat, tõenäolist ja konservatiivset hinnangut. Konservatiivne prognoos ei ole automaatselt garanteeritud arve ülempiir. Päringupiirid, väljundipiirid, retry-reserv ja täitmisaegne eelarvekontroll peavad seda toetama.

### 4.3 Hinnakirja andmemudel ja krediidid

- Eraldi versioonitud hinnakataloog: teenusepakkuja, endpointi tüüp, täpne mudel, ühikud, valuuta, sisend/väljund/audio/embedding, kehtivus, allikas ja viimase kontrolli kuupäev.
- Mitte parsida hindu `types.ts` kasutajaliidese tekstist. Kontrollida iga toetatud mudeli hinnad ja pildiarvestus ametlikust dokumentatsioonist implementeerimise ajal.
- Tundmatu või kohandatud mudel: „hind teadmata”, võimalus sisestada tariif, või ainult päringute/kaadrite piirang. Mitte kuvada 0 €.
- Esmalt näidata arveldusvaluutat. EUR teisendus vajab kuupäevaga kurssi ning märget, et maksud ja tegelik arvelduskurss võivad erineda.
- Krediidid ei ole teenusepakkujate vahel universaalne ühik. Näidata neid ainult teadaoleva kasutajakonto või lepingulise teisenduse korral. Kontojääki ei saa tuletada video suurusest ega kohalikust kuluajaloost.
- Kohaliku mudeli juures näidata eraldi API-tasu puudumist ja ajakulu; mitte esitada kogu arvutikasutust tasuta teenusena.

### 4.4 Näidis, mida kasutaja peaks nägema

```text
Kaust: 100 videofaili, neist 80 kinnitatud unikaalset
Juba analüüsitud: 20; selles töös: 60 videot
Kogukestus / valitud katvus: ... / ...
Kaadreid: ...; vision-päringuid: ...; embeddingupäringuid: ...
Kõnet: ... min; väljasaadetav maht ligikaudu ... MB
Mudel ja profiil: ...
Eeldatav hind: [alumine–ülemine] USD; tõenäoline: ... USD
Hinna alus: [hinnakirja kuupäev]; kindlus: esialgne / kalibreeritud
Eeldatav aeg: [vahemik]; eelarve: [kasutaja summa]
[Muuda profiili] [Alusta] [Loobu]
```

Numbrilised hinnad jäetakse selles plaanis teadlikult välja: neid pole kasutaja kausta ega kontrollitud hinnakirja põhjal arvutatud.

## 5. Töökindlus ja eelarve täitmise ajal

### Tulemuste säilitamine

- Esimene väike parandus: üks SQLite-kirjutaja loeb tulemuste kanalit töötajate jooksmise ajal. „Salvestatud” loendur suureneb alles eduka transaktsiooni järel.
- Seejärel lisada püsivad `analysis_runs`, `analysis_items` ja kaadripakkide kontrollpunktid. Olekud vähemalt pending/running/partial/completed/failed/cancelled.
- Salvestada vision-kirjeldused enne heli/embeddingute sammu. Nende sammude viga ei tohi nõuda tasulise vision-analüüsi kordamist.
- Iga plaanitud ajatempel või helilõik peab omama õnnestumise/viga/puudumise olekut. „Valmis” tähendab plaani kaetust, mitte ühe annotatsiooni olemasolu.
- Kordusanalüüs luua uue tulemusversioonina; varasem edukas versioon jääb kasutatavaks, kuni uus on kinnitatud. Säilitada teiste mudelite ajalugu ja olemasolevate SQLite-andmete migratsioon.

### Kuluregister ja piiramine

- Salvestada iga katse kohta töö/paki ID, mudel, operatsioon, ajakulu, tulemus, teenuse request ID, raporteeritud kasutus, hinnakirja versioon ja arvutatud summa. Salajasi võtmeid ega meediasisu logidesse ei kirjutata.
- Ka vigase JSON-i või katkise analüüsivastusega päring võib olla arveldatud: kasutusinfo registreerida enne sisulist parsimist. Võrgutimeout pärast saatmist märgistada võimaliku teadmata kuluna.
- Enne iga päringut reserveerida eelarvest konservatiivne summa aatomiliselt kõigi töötajate peale. Võrrelda: teadaolev kulu + aktiivsed reserveeringud + järgmine päring ≤ limiit.
- Retry vajab uut kontrolli; ebakindla varasema katse võimalikku kulu ei tohi vaikselt nullida. Stopi järel ei alustata uusi katseid.
- Näidata töö ajal kasutust, eelarvet, aktiivsete päringute reserveeringut ja järelejäänud hinnangut. Juba teenusesse saadetud päringu tasu ei saa kasutajaliidese stopiga tagasi võtta.
- Kohalik piirang ei kontrolli sama API-võtmega teistes rakendustes tehtud tööd. Arve lõplik tõde jääb teenusepakkujale; raporteeritud kasutusel arvutatud kulu eristada kinnitatud arvest.
- Otsingu embeddingud ja „Test connection” päringud kajastada eraldi operatsioonidena, et kuluajalugu hõlmaks ka analüüsivälist kasutust.

## 6. Kiiruse parandamise plaan

1. **Mõõdikud enne optimeerimist.** Eraldi discovery, identiteedikontroll, FFprobe, dekodeerimine, pildi kodeerimine, vision, heli väljavõte, transkriptsioon, embedding, retry ooteaeg ja SQLite commit. Fikseerida andmekandja, videokodek/resolutsioon, kestus, faili-/kaadriarv, mudel, paralleelsus ja seaded. Logida ühikud ja ajad, mitte privaatne sisu.
2. **Uus ETA.** Ennustada etappe sobivate ühikute järgi: dekodeeritavad sekundid, kaadrid/pakid, audiominutid, embeddingutekstid. Arvestada paralleelsete töötajate kriitilist teed; kuvada vahemik ja mõõtmiste arv. Katkestatud/vigaseid töid ei kasutata edukate tööde tavakiiruse näidistena.
3. **Vähendada kordustööd.** Püsivad vahetulemused, ainult ebaõnnestunud pakkide retry ning tekstist uue embeddingu arvutamine ilma piltide uuesti saatmiseta. Eraldi visual-pipeline’i ja embeddinguruumi versioonid.
4. **Piiratud tööjärjekorrad.** Dekodeerimise, võrgu ja DB jaoks eraldi järjekorrad, mälu ülempiir ning vastusurve. Praegune pilve paralleelsus on juba kaks faili; seda ei suurendata pimesi.
5. **Embeddingute koondamine.** Esmalt lahendada Gemini järjestikused üksikpäringud toetatud API võimaluste järgi; toetuse puudumisel kasutada väikest piiratud paralleelsust. Säilitada seos teksti ID ja vektori vahel ning mudeli sisendipiirangud.
6. **Kaadristrateegiad.** „Kiirülevaade” jaotab piiratud kaadrid üle kogu video, „Tasakaalustatud” kasutab kontrollitud intervalli ning „Detailne” tihendab kasutaja valitud lõike. Kuvada igaühe hind ja katvus. Stseenivahetus ja sarnaste kaadrite vähendamine alles pärast baasvõrdlust; OCR-i ja lühikesi sündmusi ei tohi vaikimisi eemaldada.
7. **FFmpeg.** Võrrelda praegust järjestikust dekodeerimist hõreda otsingupõhise väljavõttega. Mõõta kodekite, pikkuste ja ketaste kaupa; juhuslik seek ei ole alati kiirem. Teha protsess katkestatavaks, koristada ajutised failid ka vigadel ning kontrollida päris ajatemplite vastavust.
8. **Suure arhiivi otsing.** Kausta- ja identiteedifilter võimalikult vara SQL-is, query-embeddingu cache mudeli järgi, vähem JSON-vektorite kordusparsimist. Vektorindeksi vajadus otsustada mõõdetud mahu põhjal.

## 7. Teostamise järjekord ja vastuvõtukriteeriumid

Mahud on ligikaudsed ühe arendaja aktiivsed tööpäevad, mitte lubatud tähtajad. Testimine ja dokumentatsioon kuuluvad iga sammu sisse; teadmata teenuse- ja migratsioonierisused võivad mahtu suurendada.

| Etapp | Maht | Konkreetne tulemus ja vastuvõtt |
| --- | --- | --- |
| A. Baasmõõtmine | 1–2 p | Korratav võrdlus vähemalt lühikeste, pikkade ja paljude väikeste failidega; etappide ajad ning paigaldatud versioon fikseeritud. Tavaline mõõtmine saab kasutada stub-teenust, tasuline kalibreerimine on nähtava piiriga. |
| B. Tulemused kohe kettale | 1–2 p | Kahefaililises katses lõpetab esimene klipp, teine töötab veel; protsessi sundlõpetamise järel on esimese tulemused taastatavad. UI loeb ainult commititud klippe. |
| C. Faili identiteedi parandamine | 2–4 p | Kiirfingerprint eraldi täielikust räsi-identiteedist; duplikaadi vahelejätmine vajab kinnitatud identiteeti. Migratsioon säilitab olemasolevad seosed ja märgib vana identiteedi kontrollimata oleku. |
| D. Ühine plaan + rahaline eelhinnang | 3–5 p | Hinnakataloog, konkreetsed kaadriplaanid, failide/kestuse/katvuse ja hinna dialoog. Plaani arvutamisel 0 tasulist päringut. Mudeli, heli või intervalli muutus uuendab hinnangut. |
| E. Kasutuse register + eelarve | 3–5 p | Kõik päringuliigid ja katsed kirjendatud; kaks töötajat ei saa sama vaba eelarvet topelt kasutada. Tundmatu hind või kasutus on nähtav. Sobimatu plaan ei käivitu vana kinnitusega. |
| F. Osaline jätkamine | 3–5 p | Vision-, heli- ja embeddingu etapid taastatavad; ühe paki viga ei kustuta teisi; uus käivitus saadab ainult puuduva töö. Täielikkus põhineb plaanil. |
| G. Läbilaskevõime ja ETA | 3–5 p | Mõõdetud embeddingu/FFmpegi/järjekorra parandused; katkestamine jõuab retryni; ETA põhineb etappidel. Siht: valitud võrdluskomplektil vähemalt 25% lühem aeg või selgelt tõendatud järgmine pudelikael, ilma kvaliteedilanguseta. |
| H. Katvus, otsing ja väljalase | 3–5 p | Kogu video kiirülevaade, valitud lõikude täpsustamine, otsingu mahutest, uuendatud dokumentatsioon ja Windowsi manuaalne testmaatriks. |

Praktiline esimene tarnitav lõik on B + D: tulemused säilivad ja hind on enne käivitamist nähtav. E peab järgnema enne, kui toodet kirjeldatakse kululimiidiga kaitstuna. C on eraldi P0 parandusrada; A annab G jaoks alusandmed. F tuleb enne suuremat paralleelsuse tõstmist.

### Pakutavad arendusülesanded

1. Etapiajastused ja korratav jõudluse võrdluskomplekt.
2. Tulemuste jooksvalt salvestamine ning tõene „salvestatud” edenemine.
3. Kiirfingerprinti ja kinnitatud sisuidentiteedi eraldamine koos migratsiooniga.
4. Versioonitud analüüsiplaan ja Rusti seadete valideerimine.
5. Hinnakataloog, hinnavahemik ja analüüsi eelvaatedialoog.
6. Päringupõhine kasutusregister ning aatomiline eelarvereserveering.
7. Kontrollpunktid, osaline olek, vigade nähtavus ja jätkamine.
8. Katkestatav retry/FFmpeg ning piiratud embeddingutöötlus.
9. Kogu video katvus ja valitud ajavahemiku süvaanalüüs.
10. Otsingu mõõtmine, dokumentatsiooni kooskõla ja väljalaske kontroll.

Need on ülesannete ettepanekud; GitHubi issue’sid ega PR-e selle ülevaatuse käigus ei loodud.

## 8. Olulised testid

- Hinnang: duplikaadid, varem analüüsitud failid, metadata puudumine, helita fail, kaadriarv 1/8/9, väga lühike ja väga pikk video, tundmatu hind, aegunud hinnakiri ning null/ülisuur seadistus.
- Identiteet: kaks üle 16 MiB faili sama alguse/keskosa/lõpu ja suurusega, kuid muu erineva sisuga, peavad saama erineva kinnitatud identiteedi.
- Krahh ja restart: valmis klipp säilib teise töötamise ajal; vision-tulemus säilib embeddingu vea järel; vana edukas analüüs säilib ebaõnnestunud kordusanalüüsi ajal.
- Eelarve: paralleelsed reserveeringud, 429/5xx, timeout pärast saatmist, vigane vastus koos kasutusinfoga, puuduv kasutusinfo ja stop retry ooteajal.
- Plaani vastavus: teostus kasutab kinnitatud failivalikut, seadeid ja kaadriaegu. Muutunud fail, seaded või hinnakiri tekitab uue plaani, kui kulu/ulatus muutub.
- Kvaliteet: video lõpu sündmus, lühike tegevus, ekraanitekst ja dialoog. Mõõta leidmist märgendatud näidetel, mitte ainult ilusat genereeritud kirjeldust.
- Jõudlus: 100, 1000 ja 10 000 klipi metadata-/otsingukatsed sünteetiliste andmetega; AI-etappide mõõtmiseks eraldi väike meediakomplekt. Kogu suurt komplekti ei saadeta tasulisele teenusele.

## 9. Dokumentatsiooni ja arhitektuuri järgmised parandused

- `architecture.md` kirjeldab pilve tööjärjekorda, kuid praegune desktopi AI töötab otse Rustist teenusepakkujasse ja salvestab SQLite’i. Lisada eraldi praeguse ning kavandatud töövoo diagramm ja andmete omandi kirjeldus.
- `security-and-cost.md` täiendada hinnangu, reserveeringute ja katkestamise tegeliku semantikaga; parandada kohese salvestamise mulje enne teostuse parandust.
- `local-index.md` ja identiteedi ADR viia kokku tegeliku fingerprinti/migratsiooniplaaniga. SQLite AI-andmeid ei saa kasutaja vaatenurgast käsitleda kergekäeliselt taastatava cache’ina, sest taastamine võib maksta raha.
- `testing-strategy.md`, `roadmap.md` ja `next-agent-plan.md` vajavad olemasolevale desktopi MVP-le vastavat uut järjekorda. Kaks ADR-i kasutavad numbrit 0005; korrastada nummerdus koos linkidega.
- Lisada andmebaasi varundamise/taastamise ja analüüsiajaloo ekspordi plaan. Enne skeemimigratsiooni kontrollida taastamist, mitte ainult migratsiooni edukat lõppu.
- `main.ts` ja `ai.rs` on koondanud palju vastutusi. Uute funktsioonide jaoks eraldada planeerija, hinnastaja, kasutusregister ja töö käivitaja; kogu UI ümberkirjutamist pole selleks vaja.

Pilvesünk, AWS-i laiendamine ja uued mudelivalikud jäävad olemasolevasse teekaarti, kuid kasutaja praeguse probleemi lahendamiseks on esmatähtsad mõõdetav kohalik töövoog, hinna nähtavus, usaldusväärne jätkamine ja korrektne analüüsi katvus.
