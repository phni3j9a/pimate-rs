package dev.pimate.app

import androidx.activity.compose.setContent
import androidx.compose.runtime.*
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*

class QuestionContract {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test
    fun displaysAndSubmitsEachRequestShape() {
        var request by
            mutableStateOf(
                JSONObject(
                    """{"kind":"select","title":"選択","message":"説明","options":["第一案","第二案"]}"""
                )
            )
        var result = JSONObject()
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) { key(request) { QuestionCard(request, true) { result = it } } }
            }
        }
        ui.onNodeWithText("説明").assertIsDisplayed()
        ui.onNodeWithText("回答する").assertIsNotEnabled()
        ui.onNodeWithText("第二案").performClick()
        ui.onNodeWithText("回答する").performClick()
        assertEquals("第二案", result.getString("value"))
        ui.runOnIdle {
            request = JSONObject("""{"kind":"input","title":"入力","placeholder":"理由を入力"}""")
        }
        ui.onNodeWithText("理由を入力").assertExists()
        ui.onNode(hasSetTextAction()).performTextInput("理由")
        ui.onNodeWithText("回答する").performClick()
        assertEquals("理由", result.getString("value"))
        ui.runOnIdle {
            request =
                JSONObject(
                    """{"kind":"editor","title":"編集","message":"初期本文","tuiStaysOpen":true}"""
                )
        }
        ui.onNodeWithText("初期本文").assertExists()
        ui.onNodeWithText("PC のエディタ", substring = true).assertExists()
        ui.onNodeWithText("キャンセル").performClick()
        assertTrue(result.getBoolean("cancelled"))
        ui.runOnIdle {
            request =
                JSONObject(
                    """{"kind":"questionnaire","title":"アンケート","message":"背景","questions":[{"question":"作業内容","multiSelect":true,"options":[{"label":"読む","description":"仕様を確認"}]}]}"""
                )
        }
        ui.onNodeWithText("背景").assertExists()
        ui.onNodeWithText("作業内容", substring = true).assertExists()
        ui.onNodeWithText("当てはまるものをすべて選択").assertExists()
        ui.onNodeWithText("仕様を確認").performClick()
        ui.onNode(hasSetTextAction()).performTextInput("補足")
        ui.onNodeWithText("回答する").performClick()
        val a = result.getJSONArray("answers").getJSONObject(0)
        assertEquals("読む", a.getJSONArray("selected").getString(0))
        assertEquals("補足", a.getString("custom"))
    }
}
