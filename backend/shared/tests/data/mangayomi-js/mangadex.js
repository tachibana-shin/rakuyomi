const mangayomiSources = [{
    "name": "MangaDex",
    "langs": ["ar", "bn", "bg", "my", "ca", "zh", "zh-hk", "cs", "da", "nl", "en", "tl", "fi", "fr", "de", "el", "he", "hi", "hu", "id", "it", "ja", "kk", "ko", "la", "lt", "ms", "mn", "ne", "no", "fa", "pl", "pt-br", "pt", "ro", "ru", "sh", "es-419", "es", "sv", "ta", "th", "tr", "uk", "vi"],
    "ids": {
        "ar": 202373705,
        "bn": 860658373,
        "bg": 722270529,
        "my": 978675083,
        "ca": 689496451,
        "zh": 593575397,
        "zh-hk": 115179159,
        "cs": 869144666,
        "da": 846142909,
        "nl": 841149659,
        "en": 810342358,
        "tl": 309024312,
        "fi": 164642544,
        "fr": 545017689,
        "de": 110023605,
        "el": 767687578,
        "he": 511907642,
        "hi": 986826068,
        "hu": 128441350,
        "id": 183977130,
        "it": 127887438,
        "ja": 204112007,
        "kk": 1063442064,
        "ko": 898061477,
        "la": 387646759,
        "lt": 270482698,
        "ms": 284400542,
        "mn": 525041874,
        "ne": 613632949,
        "no": 441032670,
        "fa": 693311514,
        "pl": 683661227,
        "pt-br": 417850874,
        "pt": 1027115198,
        "ro": 399589398,
        "ru": 367421943,
        "sh": 254140838,
        "es-419": 823535267,
        "es": 736630443,
        "sv": 146351677,
        "ta": 739930809,
        "th": 385031783,
        "tr": 1008587213,
        "uk": 778357609,
        "vi": 88174952
    },
    "baseUrl": "https://mangadex.org",
    "apiUrl": "https://api.mangadex.org",
    "iconUrl": "https://raw.githubusercontent.com/m2k3a/mangayomi-extensions/main/javascript/icon/all.mangadex.png",
    "typeSource": "single",
    "itemType": 0,
    "version": "0.1.4",
    "pkgPath": "manga/src/all/mangadex.js"
}];

class DefaultExtension extends MProvider {
    constructor() {
        super();
        this.client = new Client();
    }
    getHeaders(url) {
        return {
            "user-agent": this.getPreference("custom_user_agent"),
        };
    }
    async getPopular(page) {
        const offset = 20 * (page - 1);
        const url = `${this.source.apiUrl}/manga?limit=20&offset=${offset}&availableTranslatedLanguage[]=${this.source.lang}&includes[]=cover_art&contentRating[]=safe&contentRating[]=suggestive${this.preferenceOriginalLanguages()}&order[followedCount]=desc`;
        const response = await this.client.get(url, this.getHeaders());
        return this.mangaRes(response.body);
    }
    async getLatestUpdates(page) {
        const offset = 20 * (page - 1);
        const url = `${this.source.apiUrl}/chapter?limit=20&offset=${offset}&translatedLanguage[]=${this.source.lang}&includeFutureUpdates=0&order[publishAt]=desc&includeFuturePublishAt=0&includeEmptyPages=0`;
        const response = await this.client.get(url, this.getHeaders());
        const mangaIds = Array.from(
            new Set(
                JSON.parse(response.body).data
                    .flatMap(item => item.relationships)
                    .filter(relationship => relationship.type === "manga")
                    .map(mangaData => mangaData.id)
            )
        );
        const mangaIdss = mangaIds.map(id => `&ids[]=${id}`).join("");
        const newUrl = `${this.source.apiUrl}/manga?includes[]=cover_art&limit=${mangaIds.length}&contentRating[]=safe&contentRating[]=suggestive${this.preferenceOriginalLanguages()}${mangaIdss}`;
        const newResponse = await this.client.get(newUrl, this.getHeaders());
        return this.mangaRes(newResponse.body);
    }
    async search(query, page, filters) {
        let offset = 20 * (page - 1);
        let url = `${this.source.apiUrl}/manga?includes[]=cover_art&offset=${offset}&limit=20&title=${query}`;

        filters.forEach(filter => {
            if (filter.type === "HasAvailableChaptersFilter") {
                if (filter.state) {
                    url += `${this.ll(url)}hasAvailableChapters=true`;
                    url += `${this.ll(url)}availableTranslatedLanguage[]=${this.source.lang}`;
                }
            } else if (filter.type === "OriginalLanguageList") {
                const langs = filter.state.filter(e => e.state);
                langs.forEach(lang => {
                    url += `${this.ll(url)}${lang.value}`;
                });
            } else if (filter.type === "ContentRatingList") {
                const ratings = filter.state.filter(e => e.state);
                ratings.forEach(rating => {
                    url += `${this.ll(url)}${rating.value}`;
                });
            } else if (filter.type === "DemographicList") {
                const demographics = filter.state.filter(e => e.state);
                demographics.forEach(demographic => {
                    url += `${this.ll(url)}${demographic.value}`;
                });
            } else if (filter.type === "StatusList") {
                const statuses = filter.state.filter(e => e.state);
                statuses.forEach(status => {
                    url += `${this.ll(url)}${status.value}`;
                });
            } else if (filter.type === "SortFilter") {
                const value = filter.state.ascending ? "asc" : "desc";
                url += `${this.ll(url)}order[${filter.values[filter.state.index].value}]=${value}`;
            } else if (filter.type === "TagsFilter") {
                filter.state.forEach(tag => {
                    url += `${this.ll(url)}${tag.values[tag.state].value}`;
                });
            } else if (filter.type === "FormatFilter") {
                const included = filter.state.filter(e => e.state === 1);
                const excluded = filter.state.filter(e => e.state === 2);
                included.forEach(val => {
                    url += `${this.ll(url)}includedTags[]=${val.value}`;
                });
                excluded.forEach(val => {
                    url += `${this.ll(url)}excludedTags[]=${val.value}`;
                });
            } else if (filter.type === "GenreFilter" || filter.type === "ThemeFilter") {
                const included = filter.state.filter(e => e.state === 1);
                const excluded = filter.state.filter(e => e.state === 2);
                included.forEach(val => {
                    url += `${this.ll(url)}includedTags[]=${val.value}`;
                });
                excluded.forEach(val => {
                    url += `${this.ll(url)}excludedTags[]=${val.value}`;
                });
            }
        });

        const response = await this.client.get(url, this.getHeaders());
        return this.mangaRes(response.body);
    }
    async getDetail(url) {
        const detailUrl = `${this.source.apiUrl}${url}?includes[]=cover_art&includes[]=author&includes[]=artist`;
        const response = await this.client.get(detailUrl, this.getHeaders());
        const data = JSON.parse(response.body).data;
        const manga = {};
        const coverRel = data.relationships.find(rel => rel.type === "cover_art");
        if (coverRel && coverRel.attributes && coverRel.attributes.fileName) {
            manga.imageUrl = `https://uploads.mangadex.org/covers/${data.id}/${coverRel.attributes.fileName}`;
        }
        const authors = data.relationships
            .filter(rel => rel.type === "author")
            .map(rel => rel.attributes.name);
        manga.author = authors.join(", ");
        manga.description = data.attributes.description[this.source.lang] ?? data.attributes.description.en ?? "";
        manga.genre = data.attributes.tags.map(tag => tag.attributes.name.en);
        if (data.attributes.contentRating && data.attributes.contentRating !== "safe") {
            manga.genre.push(data.attributes.contentRating);
        }
        if (data.attributes.publicationDemographic && data.attributes.publicationDemographic !== "null") {
            manga.genre.push(data.attributes.publicationDemographic);
        }
        manga.status = { "ongoing": 0, "completed": 1, "hiatus": 2, "cancelled": 3 }[data.attributes.status];
        const mangaId = url.split("/").pop();
        const chapterData = await this.fetchPaginatedChapters(mangaId, this.source.lang);

        manga.chapters = chapterData;
        return manga;
    }
    async fetchPaginatedChapters(mangaId, lang) {
        const chapters = [];
        let offset = 0;
        let hasMoreResults = true;

        while (hasMoreResults) {
            const url = `${this.source.apiUrl}/manga/${mangaId}/feed?limit=500&offset=${offset}&includes[]=user&includes[]=scanlation_group&order[volume]=desc&order[chapter]=desc&translatedLanguage[]=${lang}&includeFuturePublishAt=0&includeEmptyPages=0&contentRating[]=safe&contentRating[]=suggestive`;
            const res = await this.client.get(url, this.getHeaders());
            const paginatedData = JSON.parse(res.body);
            const limit = paginatedData?.limit ?? 0;
            const total = paginatedData?.total ?? 0;
            const newChapters = this.extractChapters(paginatedData);
            chapters.push(...newChapters);
            offset += limit;
            hasMoreResults = offset < total;
        }

        return chapters;
    }

    extractChapters(paginatedData) {
        const chaptersList = [];

        const dataList = paginatedData.data ?? [];
        for (const res of dataList) {
            let scan = "";
            const groups = res?.relationships?.filter(
                rel => rel.id !== "00e03853-1b96-4f41-9542-c71b8692033b"
            );
            for (const group of groups) {
                const groupData = group?.attributes ?? {};
                const name = groupData?.name ?? "";
                if (name) {
                    scan += name;
                }
                if (scan === "") {
                    const username = groupData?.username ?? "";
                    if (username) {
                        scan += `Uploaded by ${username}`;
                    }
                }
            }
            if (scan === "") {
                scan = "No Group";
            }
            const attributes = res?.attributes ?? {};
            const volume = attributes?.volume;
            const chapter = attributes?.chapter;
            let title = attributes?.title ?? "";
            if (volume === null && chapter === null && title === "") {
                title = "Oneshot"
            }
            const chapName = `${volume ? `Vol.${volume} ` : ""}${chapter ? `Ch.${chapter} ` : ""}${title}`;
            chaptersList.push({
                name: chapName,
                url: res?.id ?? "",
                scanlator: scan,
                dateUpload: new Date(attributes?.publishAt).valueOf().toString(),
            });

        }

        return chaptersList;
    }
    async getPageList(url) {
        const pageUrl = `${this.source.apiUrl}/at-home/server/${url}`;
        const response = await this.client.get(pageUrl, this.getHeaders());
        const bodyJson = JSON.parse(response.body);
        const host = bodyJson.baseUrl;
        const chapter = bodyJson.chapter;
        const hash = chapter.hash;
        const chapterDatas = chapter.data;
        return chapterDatas.map(file => `${host}/data/${hash}/${file}`);
    }
    getFilterList() {
        throw new Error("getFilterList not implemented");
    }
    mangaRes(res) {
        const data = JSON.parse(res).data;
        return {
            list: data.map(e => ({
                name: this.findTitle(e, this.source.lang),
                imageUrl: this.getCover(e),
                link: `/manga/${e.id}`,
            })),
            hasNextPage: true
        };
    }
    findTitle(data, lang) {
        const titles = data.attributes.title;
        const altTitles = data.attributes.altTitles || [];
        if (titles[lang]) return titles[lang];
        if (titles.en) return titles.en;
        const altInLang = altTitles.find(t => t[lang]);
        if (altInLang) return altInLang[lang];
        const altInEn = altTitles.find(t => t.en);
        if (altInEn) return altInEn.en;
        return "";
    }
    getCover(data) {
        const coverArt = data.relationships?.find(r => r.type === "cover_art");
        return coverArt ? `https://uploads.mangadex.org/covers/${data.id}/${coverArt.attributes.fileName}` : "";
    }
    preferenceOriginalLanguages() {
        const originalLanguages = this.getPreference("original_languages", []);
        return originalLanguages.length ? `&${originalLanguages.join("&")}` : "";
    }
    getPreference(key, defaulValue) {
        const preferences = new SharedPreferences();
        return preferences.get(key, defaulValue);
    }
    ll(url) {
        return url.includes("?") ? "&" : "?";
    }
    getSourcePreferences() {
        return [
            {
                "key": "cover_quality",
                "listPreference": {
                    "title": "Cover quality",
                    "summary": "Select the quality of the covers to load",
                    "valueIndex": 0,
                    "entries": [
                        "Original",
                        "Medium",
                        "Low"],
                    "entryValues": [
                        "",
                        ".512.jpg",
                        ".256.jpg"],
                }
            },
            {
                "key": "original_languages",
                "multiSelectListPreference": {
                    "title": "Filter original languages",
                    "summary": "Only show content that was originaly published in the selected languages in both latest and browse",
                    "entries": [
                        "Japanese",
                        "Chinese",
                        "Korean"],
                    "entryValues": [
                        "originalLanguage[]=ja",
                        "originalLanguage[]=zh&originalLanguage[]=zh-hk",
                        "originalLanguage[]=ko"],
                    "values": []
                }
            },
            {
                "key": "custom_user_agent",
                "editTextPreference": {
                    "title": "Set custom User-Agent",
                    "summary": "",
                    "value": "Dalvik/2.1.0 (Linux; U; Android 14; 22081212UG Build/UKQ1.230917.001)",
                    "dialogTitle": "Set custom User-Agent",
                    "dialogMessage": "Specify a custom user agent",
                }
            }
        ];
    }
    getFilterList() {
        return [
            {
                type_name: "CheckBox",
                type: "HasAvailableChaptersFilter",
                name: "Has available chapters",
                value: ""
            },
            {
                type_name: "GroupFilter",
                type: "OriginalLanguageList",
                name: "Original language",
                state: [
                    ["Japanese (Manga)", "originalLanguage[]=ja"],
                    ["Chinese (Manhua)",
                        "originalLanguage[]=zh&originalLanguage[]=zh-hk"],
                    ["Korean (Manhwa)", "originalLanguage[]=ko"]
                ].map(x => ({ type_name: 'CheckBox', name: x[0], value: x[1] }))
            },
            {
                type_name: "GroupFilter",
                type: "ContentRatingList",
                name: "Content rating",
                state: [
                    ["Safe", "contentRating[]=safe"],
                    ["Suggestive", "contentRating[]=suggestive"]
                ].map(x => ({ type_name: 'CheckBox', name: x[0], value: x[1], state: true }))
            },
            {
                type_name: "GroupFilter",
                type: "DemographicList",
                name: "Publication demographic",
                state: [
                    ["None", "publicationDemographic[]=none"],
                    ["Shounen", "publicationDemographic[]=shounen"],
                    ["Shoujo", "publicationDemographic[]=shoujo"],
                    ["Seinen", "publicationDemographic[]=seinen"],
                    ["Josei", "publicationDemographic[]=josei"]
                ].map(x => ({ type_name: 'CheckBox', name: x[0], value: x[1] }))
            },
            {
                type_name: "GroupFilter",
                type: "StatusList",
                name: "Status",
                state: [
                    ["Ongoing", "status[]=ongoing"],
                    ["Completed", "status[]=completed"],
                    ["Hiatus", "status[]=hiatus"],
                    ["Cancelled", "status[]=cancelled"]
                ].map(x => ({ type_name: 'CheckBox', name: x[0], value: x[1] }))
            },
            {
                type_name: "SortFilter",
                type: "SortFilter",
                name: "Sort",
                state: {
                    type_name: "SortState",
                    index: 5,
                    ascending: false
                },
                values: [
                    ["Alphabetic", "title"],
                    ["Chapter uploded at", "latestUploadedChapter"],
                    ["Number of follows", "followedCount"],
                    ["Content created at", "createdAt"],
                    ["Content info updated at", "updatedAt"],
                    ["Relevance", "relevance"],
                    ["Year", "year"],
                    ["Rating", "rating"]
                ].map(x => ({ type_name: 'SelectOption', name: x[0], value: x[1] }))
            },
            {
                type_name: "GroupFilter",
                type: "TagsFilter",
                name: "Tags mode",
                state: [
                    {
                        type_name: "SelectFilter",
                        type: "TagInclusionMode",
                        name: "Included tags mode",
                        state: 0,
                        values: [
                            ["AND", "includedTagsMode=AND"],
                            ["OR", "includedTagsMode=OR"]
                        ].map(x => ({ type_name: 'SelectOption', name: x[0], value: x[1] }))
                    },
                    {
                        type_name: "SelectFilter",
                        type: "TagExclusionMode",
                        name: "Excluded tags mode",
                        state: 1,
                        values: [
                            ["AND", "excludedTagsMode=AND"],
                            ["OR", "excludedTagsMode=OR"]
                        ].map(x => ({ type_name: 'SelectOption', name: x[0], value: x[1] }))
                    }]
            },
            {
                type_name: "GroupFilter",
                type: "ContentsFilter",
                name: "Content",
                state: [
                    ["Gore", "b29d6a3d-1569-4e7a-8caf-7557bc92cd5d"],
                    ["Sexual Violence", "97893a4c-12af-4dac-b6be-0dffb353568e"]
                ].map(x => ({ type_name: 'TriState', name: x[0], value: x[1] }))
            },
            {
                type_name: "GroupFilter",
                type: "FormatFilter",
                name: "Format",
                state: [
                    ["4-Koma", "b11fda93-8f1d-4bef-b2ed-8803d3733170"],
                    ["Adaptation", "f4122d1c-3b44-44d0-9936-ff7502c39ad3"],
                    ["Anthology", "51d83883-4103-437c-b4b1-731cb73d786c"],
                    ["Award Winning", "0a39b5a1-b235-4886-a747-1d05d216532d"],
                    ["Doujinshi", "b13b2a48-c720-44a9-9c77-39c9979373fb"],
                    ["Fan Colored", "7b2ce280-79ef-4c09-9b58-12b7c23a9b78"],
                    ["Full Color", "f5ba408b-0e7a-484d-8d49-4e9125ac96de"],
                    ["Long Strip", "3e2b8dae-350e-4ab8-a8ce-016e844b9f0d"],
                    [
                        "Official Colored", "320831a8-4026-470b-94f6-8353740e6f04"],
                    ["Oneshot", "0234a31e-a729-4e28-9d6a-3f87c4966b9e"],
                    ["User Created", "891cf039-b895-47f0-9229-bef4c96eccd4"],
                    ["Web Comic", "e197df38-d0e7-43b5-9b09-2842d0c326dd"]
                ].map(x => ({ type_name: 'TriState', name: x[0], value: x[1] }))
            },
            {
                type_name: "GroupFilter",
                type: "GenreFilter",
                name: "Genre",
                state: [
                    ["Action", "391b0423-d847-456f-aff0-8b0cfc03066b"],
                    ["Adventure", "87cc87cd-a395-47af-b27a-93258283bbc6"],
                    ["Boys' Love", "5920b825-4181-4a17-beeb-9918b0ff7a30"],
                    ["Comedy", "4d32cc48-9f00-4cca-9b5a-a839f0764984"],
                    ["Crime", "5ca48985-9a9d-4bd8-be29-80dc0303db72"],
                    ["Drama", "b9af3a63-f058-46de-a9a0-e0c13906197a"],
                    ["Fantasy", "cdc58593-87dd-415e-bbc0-2ec27bf404cc"],
                    ["Girls' Love", "a3c67850-4684-404e-9b7f-c69850ee5da6"],
                    ["Historical", "33771934-028e-4cb3-8744-691e866a923e"],
                    ["Horror", "cdad7e68-1419-41dd-bdce-27753074a640"],
                    ["Isekai", "ace04997-f6bd-436e-b261-779182193d3d"],
                    ["Magical Girls", "81c836c9-914a-4eca-981a-560dad663e73"],
                    ["Mecha", "50880a9d-5440-4732-9afb-8f457127e836"],
                    ["Medical", "c8cbe35b-1b2b-4a3f-9c37-db84c4514856"],
                    ["Mystery", "ee968100-4191-4968-93d3-f82d72be7e46"],
                    ["Philosophical", "b1e97889-25b4-4258-b28b-cd7f4d28ea9b"],
                    ["Psychological", "3b60b75c-a2d7-4860-ab56-05f391bb889c"],
                    ["Romance", "423e2eae-a7a2-4a8b-ac03-a8351462d71d"],
                    ["Sci-Fi", "256c8bd9-4904-4360-bf4f-508a76d67183"],
                    ["Slice of Life", "e5301a23-ebd9-49dd-a0cb-2add944c7fe9"],
                    ["Sports", "69964a64-2f90-4d33-beeb-f3ed2875eb4c"],
                    ["Superhero", "7064a261-a137-4d3a-8848-2d385de3a99c"],
                    ["Thriller", "07251805-a27e-4d59-b488-f0bfbec15168"],
                    ["Tragedy", "f8f62932-27da-4fe4-8ee1-6779a8c5edba"],
                    ["Wuxia", "acc803a4-c95a-4c22-86fc-eb6b582d82a2"]
                ].map(x => ({ type_name: 'TriState', name: x[0], value: x[1] }))
            },
            {
                type_name: "GroupFilter",
                type: "ThemeFilter",
                name: "Theme",
                state: [
                    ["Aliens", "e64f6742-c834-471d-8d72-dd51fc02b835"],
                    ["Animals", "3de8c75d-8ee3-48ff-98ee-e20a65c86451"],
                    ["Cooking", "ea2bc92d-1c26-4930-9b7c-d5c0dc1b6869"],
                    ["Crossdressing", "9ab53f92-3eed-4e9b-903a-917c86035ee3"],
                    ["Delinquents", "da2d50ca-3018-4cc0-ac7a-6b7d472a29ea"],
                    ["Demons", "39730448-9a5f-48a2-85b0-a70db87b1233"],
                    ["Genderswap", "2bd2e8d0-f146-434a-9b51-fc9ff2c5fe6a"],
                    ["Ghosts", "3bb26d85-09d5-4d2e-880c-c34b974339e9"],
                    ["Gyaru", "fad12b5e-68ba-460e-b933-9ae8318f5b65"],
                    ["Harem", "aafb99c1-7f60-43fa-b75f-fc9502ce29c7"],
                    ["Loli", "2d1f5d56-a1e5-4d0d-a961-2193588b08ec"],
                    ["Mafia", "85daba54-a71c-4554-8a28-9901a8b0afad"],
                    ["Magic", "a1f53773-c69a-4ce5-8cab-fffcd90b1565"],
                    ["Martial Arts", "799c202e-7daa-44eb-9cf7-8a3c0441531e"],
                    ["Military", "ac72833b-c4e9-4878-b9db-6c8a4a99444a"],
                    ["Monster Girls", "dd1f77c5-dea9-4e2b-97ae-224af09caf99"],
                    ["Monsters", "36fd93ea-e8b8-445e-b836-358f02b3d33d"],
                    ["Music", "f42fbf9e-188a-447b-9fdc-f19dc1e4d685"],
                    ["Ninja", "489dd859-9b61-4c37-af75-5b18e88daafc"],
                    [
                        "Office Workers", "92d6d951-ca5e-429c-ac78-451071cbf064"],
                    ["Police", "df33b754-73a3-4c54-80e6-1a74a8058539"],
                    [
                        "Post-Apocalyptic", "9467335a-1b83-4497-9231-765337a00b96"],
                    ["Reincarnation", "0bc90acb-ccc1-44ca-a34a-b9f3a73259d0"],
                    ["Reverse Harem", "65761a2a-415e-47f3-bef2-a9dababba7a6"],
                    ["Samurai", "81183756-1453-4c81-aa9e-f6e1b63be016"],
                    ["School Life", "caaa44eb-cd40-4177-b930-79d3ef2afe87"],
                    ["Shota", "ddefd648-5140-4e5f-ba18-4eca4071d19b"],
                    ["Supernatural", "eabc5b4c-6aff-42f3-b657-3e90cbd00b75"],
                    ["Survival", "5fff9cde-849c-4d78-aab0-0d52b2ee1d25"],
                    ["Time Travel", "292e862b-2d17-4062-90a2-0356caa4ae27"],
                    [
                        "Traditional Games", "31932a7e-5b8e-49a6-9f12-2afa39dc544c"],
                    ["Vampires", "d7d1730f-6eb0-4ba6-9437-602cac38664c"],
                    ["Video Games", "9438db5a-7e2a-4ac0-b39e-e0d95a34b8a8"],
                    ["Villainess", "d14322ac-4d6f-4e9b-afd9-629d5f4d8a41"],
                    [
                        "Virtual Reality", "8c86611e-fab7-4986-9dec-d1a2f44acdd5"],
                    ["Zombies", "631ef465-9aba-4afb-b0fc-ea10efe274a8"]
                ].map(x => ({ type_name: 'TriState', name: x[0], value: x[1] }))
            },

        ];
    }

}
