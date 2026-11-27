package com.dec.privatemusic.player

/** Offline catalog used by rendering tests and the opt-in desktop demo mode. */
class DemoCatalogApi : CatalogApi {
    override val isDemo: Boolean = true
    private val offline = mutableListOf<OfflineTrack>()

    override fun library(offset: Int, count: Int): TrackPage = TrackPage(demoTracks.drop(offset).take(count), demoTracks.size)
    override fun search(query: String, offset: Int, count: Int): TrackPage {
        val matches = demoTracks.filter { "${it.artist} ${it.title}".contains(query, ignoreCase = true) }
        return TrackPage(matches.drop(offset).take(count), matches.size)
    }
    override fun catalogMixes(): List<MusicMix> = listOf(
        MusicMix("common", "Мой микс", "Демонстрационная подборка"),
        MusicMix("daily", "Микс дня", "Пять треков"),
    )
    override fun streamMix(id: String, options: String?, offset: Int): List<Track> = demoTracks.drop(offset)
    override fun recommendations(): List<Track> = demoTracks
    override fun newReleases(): List<Album> = listOf(Album(1, 1, "Private Music Selection", artists = listOf(ArtistRef("demo", "Private Music"))))
    override fun downloads(): List<OfflineTrack> = offline.toList()
    override fun download(track: Track): OfflineTrack {
        val item = OfflineTrack(track, "demo://${track.key}", 0, 0)
        if (offline.none { it.track.key == track.key }) offline += item
        return item
    }
    override fun removeDownload(trackId: String): Boolean = offline.removeAll { it.track.key == trackId }
    override fun lyrics(track: Track): Lyrics = Lyrics(
        source = "demo",
        text = "${track.artist} — ${track.title}\nДемонстрационный текст песни",
        lines = listOf(LyricsLine(0.0, "Демонстрационный текст песни")),
    )
    override fun addToLibrary(track: Track): Track = track
    override fun removeFromLibrary(track: Track) = Unit
    override fun setShuffle(on: Boolean) = Unit
    override fun setRepeat(repeat: String) = Unit
    override fun setEqualizer(gains: List<Float>) = Unit
    override fun outputDevices(): List<String> = listOf("Demo output")
    override fun setOutputDevice(name: String?) = Unit
}
