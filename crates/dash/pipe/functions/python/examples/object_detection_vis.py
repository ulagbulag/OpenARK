#!/usr/bin/env python3


import io
import os
from typing import Any

import cv2
import numpy as np
from PIL import Image


class FrameCursor:
    def __init__(self) -> None:
        self._current_key = None
        self._ignore_frame = os.environ.get(
            'PIPE_PYTHON_IGNORE_FRAME', 'false') == 'true'

    def update(self, key: Any) -> bool:
        if self._ignore_frame:
            return True

        if self._current_key is not None and self._current_key >= key:
            return False
        self._current_key = key
        return True


# init cursor
frame_cursor = FrameCursor()


# show window
WINDOW_NAME = 'Object Detection'
cv2.namedWindow(WINDOW_NAME, cv2.WND_PROP_FULLSCREEN)


def tick(inputs: list[Any]) -> list[Any]:
    # skip if empty inputs
    if not inputs:
        return []

    # load payloads
    input_set: list[tuple[int, int, str, str, bytes]] = [
        (
            batch_idx,
            payload_idx,
            key,
            input.reply,
            payload,
        )
        for batch_idx, input in enumerate(inputs)
        for payload_idx, (key, payload) in enumerate(input.payloads)
    ]

    # skip if empty payloads
    if not input_set:
        return []

    # load inputs
    input_images = [
        (key, Image.open(io.BytesIO(payload)))
        for (_, _, key, _, payload) in input_set
    ]

    # skip if already passed frame
    if not frame_cursor.update(input_images[0][0]):
        return []

    # draw image
    input_image = cv2.cvtColor(np.array(input_images[0][1]), cv2.COLOR_RGB2BGR)

    # draw bbox
    value = inputs[0].value.get('value', None)
    if not value is None:
        for input_object in value:
            input_object_box: dict[str, float] = input_object['box']
            input_object_color = (255, 0, 0)
            input_object_name: str = input_object['name']
            pt_min = (int(input_object_box['x1']), int(input_object_box['y1']))
            pt_max = (int(input_object_box['x2']), int(input_object_box['y2']))

            input_image = cv2.rectangle(
                input_image, pt_min, pt_max, input_object_color, 1)
            input_image = cv2.putText(input_image, input_object_name,
                                      pt_min, cv2.FONT_HERSHEY_SIMPLEX, 0.6, (255, 255, 255), 1)

    # show
    cv2.imshow(WINDOW_NAME, input_image)
    cv2.waitKey(1)

    return []
