package dev.pimate.app

import org.junit.Assert.*
import org.junit.Test

class FolderMatchTest {
    @Test fun trustMatchesOnlyTheSameFolder() {
        assertTrue(sameFolder("~/code/", "/home/me/code", "/home/me"))
        assertTrue(sameFolder("/", "/", "/home/me"))
        assertFalse(sameFolder("", "/", "/home/me"))
        assertFalse(sameFolder("~/code", "/home/other/code", "/home/me"))
        assertFalse(sameFolder(" ~/code/", "/home/me/code", "/home/me"))
    }
}
